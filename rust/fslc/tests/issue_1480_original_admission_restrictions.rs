// SPDX-License-Identifier: Apache-2.0
//! Shared located admission for original engine restrictions; no binder support lift.
#[path = "support/scratch_identity.rs"]
mod scratch_identity;
use fsl_core::{
    FsResolver, ModelError, OriginChain, OriginId, OriginSite, Span, TypeRef, build_model,
    parse_kernel_source,
};
use fsl_syntax::SourcePos;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
const INIT_STATE_RANGE: &str = r"spec InitRange {
  state {n:0..2}
  init {
    n=1
    forall z in 0..n {}
  }
  action idle() {}
}
";
const STRUCT_EXPRESSION: &str = r"spec StructExpr {
  struct Cell {flag:Bool}
  state {n:0..1}
  init {n=0}
  action idle() {}
  invariant P {forall s:Cell {s.flag or not s.flag}}
}
";
const STRUCT_STATEMENT: &str = r"spec StructStmt {
  struct Cell {flag:Bool}
  state {n:0..1}
  init {n=0}
  action idle() {
    forall s:Cell {}
  }
}
";
const STRUCT_INIT: &str = r"spec StructInit {
  struct Cell {flag:Bool}
  state {n:0..1}
  init {
    n=0
    forall s:Cell {}
  }
  action idle() {}
}
";

struct Case {
    id: &'static str,
    source: &'static str,
    message: &'static str,
    rendered: &'static str,
    position: (usize, usize, u32, u32, u32),
    typed: bool,
    init: bool,
}
const CASES: [Case; 4] = [
    Case {
        id: "init-state-range",
        source: INIT_STATE_RANGE,
        message: "init forall range bounds must be compile-time constants; state variable 'n' is not allowed",
        rendered: "init forall range bounds must be compile-time constants; state variable 'n' is not allowed",
        position: (55, 61, 5, 5, 11),
        typed: false,
        init: true,
    },
    Case {
        id: "struct-expression",
        source: STRUCT_EXPRESSION,
        message: "invalid model expression: type 'Cell' is not a finite scalar",
        rendered: "invalid model expression: type 'Cell' is not a finite scalar at 6:3",
        position: (95, 104, 6, 3, 12),
        typed: true,
        init: false,
    },
    Case {
        id: "struct-statement",
        source: STRUCT_STATEMENT,
        message: "invalid model expression: type 'Cell' is not a finite scalar",
        rendered: "invalid model expression: type 'Cell' is not a finite scalar at 6:5",
        position: (96, 102, 6, 5, 11),
        typed: true,
        init: false,
    },
    Case {
        id: "struct-init",
        source: STRUCT_INIT,
        message: "invalid init statement: type 'Cell' is not a finite scalar",
        rendered: "invalid init statement: type 'Cell' is not a finite scalar at 6:5",
        position: (82, 88, 6, 5, 11),
        typed: true,
        init: true,
    },
];
fn expected(case: &Case) -> ModelError {
    let (start, end, line, column, end_column) = case.position;
    let span = Span {
        start: SourcePos {
            offset: start,
            line,
            column,
        },
        end: SourcePos {
            offset: end,
            line,
            column: end_column,
        },
    };
    ModelError {
        message: case.message.into(),
        origin: case.typed.then(|| {
            Box::new(OriginChain {
                id: OriginId(format!("kernel:type-diagnostic:{start}:{end}")),
                dialect: "kernel".into(),
                primary: Some(OriginSite {
                    source_file: None,
                    span: Some(span),
                    dialect: "kernel".into(),
                    declaration_path: vec![],
                }),
                secondary: vec![],
                lowering_steps: vec![],
                generated: false,
            })
        }),
        span: (case.init || !case.typed).then_some(span),
        name_resolution: false,
        diagnostic_code: None,
        hint: None,
        quick_fix: None,
    }
}
fn packet(case: &Case) -> Value {
    json!({"fsl":"1.0","result":"error","kind":"semantics","message":case.rendered,
        "loc":{"line":case.position.2,"column":case.position.3},
        "versions":{"verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
        "solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}}})
}
fn fixture(case: &Case) -> PathBuf {
    let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join("1480-original-restrictions");
    std::fs::create_dir_all(&parent).unwrap();
    let directory = scratch_identity::unique(parent.join(case.id));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("model.fsl"), case.source).unwrap();
    directory
}
fn observe(directory: &Path, route: &str) -> (i32, Value) {
    let args = if route == "check" {
        vec!["check", "model.fsl"]
    } else {
        vec![
            "verify",
            "model.fsl",
            "--engine",
            route,
            "--depth",
            "2",
            "--no-cache",
        ]
    };
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(&args)
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .unwrap();
    std::fs::write(
        directory.join(format!("{route}.stdout.json")),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        directory.join(format!("{route}.stderr.txt")),
        &output.stderr,
    )
    .unwrap();
    eprintln!(
        "ORIGINAL_RAW bin={} cwd={} args={args:?} exit={:?} stdout={} stderr={}",
        env!("CARGO_BIN_EXE_fslc"),
        directory.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn original_restrictions_are_complete_located_core_errors() {
    let outputs = CASES
        .iter()
        .map(|case| {
            let parsed = parse_kernel_source(case.source, &FsResolver::new("."));
            eprintln!(
                "ORIGINAL_CORE_SOURCE id={} source={:?} parsed={parsed:#?}",
                case.id, case.source
            );
            let actual = build_model(parsed.unwrap());
            eprintln!("ORIGINAL_CORE id={} actual={actual:#?}", case.id);
            (case, actual)
        })
        .collect::<Vec<_>>();
    for (case, actual) in outputs {
        let actual = actual.expect_err(case.id);
        assert_eq!(actual, expected(case));
        assert_eq!(actual.to_string(), case.rendered);
    }
}
#[test]
fn check_and_all_three_engines_share_the_full_frontend_rejection() {
    let mut outputs = vec![];
    for repeat in 0..2 {
        for case in &CASES {
            let directory = fixture(case);
            for route in ["check", "bmc", "induction", "explicit"] {
                let actual = observe(&directory, route);
                eprintln!(
                    "ORIGINAL_PACKET repeat={repeat} id={} route={route} actual={actual:#?}",
                    case.id
                );
                outputs.push((case, actual));
            }
        }
    }
    for (case, actual) in outputs {
        assert_eq!(actual, (2, packet(case)));
        let mut wrong = packet(case);
        wrong["loc"]["column"] = json!(case.position.3 + 1);
        assert_ne!(actual, (2, wrong));
    }
}
#[test]
fn existing_supported_binders_rhs_ranges_and_struct_values_keep_admission() {
    let sources = [
        r"spec StaticInit {state {n:0..2} init {n=1 forall z in 0..1 {}} action idle(){}}",
        r"spec InitRhs {state {n:0..2,done:Bool} init {n=1 done=forall z in 0..n {z<=1}} action idle(){}}",
        r"spec ActionRange {state {n:0..2} init {n=1} action idle(){forall z in 0..n {}}}",
        r"spec PropertyRange {state {n:0..2} init {n=1} action idle(){} invariant P {forall z in 0..n {z<=2}}}",
        r"spec LexicalAction {state {n:0..2} init {n=1} action idle(){forall n in 0..1 {forall z in 0..n {}}}}",
        r"spec BoolBinders {state {n:0..2} init {n=1 forall b:Bool {}} action idle(){forall b:Bool {}} invariant P {forall b:Bool {b or not b}}}",
        r"spec Collections {type K=0..1 state {n:K} init {n=0 forall z in Set {0,1} {}} action idle(){forall z in Seq {0,1} {}} invariant P {forall z in Set {0,1} {z<=1}}}",
        r"spec StructValue {struct Cell {flag:Bool} state {n:0..1} init {n=0} action idle(){} invariant P {Cell {flag:true}==Cell {flag:true}}}",
        r"spec StructCollection {struct Cell {flag:Bool} state {n:0..1} init {n=0} action idle(){} invariant P {forall s in Set {Cell {flag:true}} {s.flag}}}",
    ];
    let outputs = sources.map(|source| {
        let actual = build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap());
        eprintln!("ORIGINAL_NEIGHBOR source={source:?} actual={actual:#?}");
        actual
    });
    let models = outputs.map(|actual| actual.expect("existing Core admission must be preserved"));
    let model = &models[0];
    assert_eq!(
        model.domain_values(&TypeRef::Bool),
        Ok(vec![
            fsl_core::FslValue::Bool(false),
            fsl_core::FslValue::Bool(true)
        ])
    );
    for ty in [
        TypeRef::Int,
        TypeRef::Set(Box::new(TypeRef::Bool)),
        TypeRef::Seq(Box::new(TypeRef::Bool), 1),
    ] {
        assert_eq!(
            model.domain_values(&ty),
            Err(ModelError {
                message: "type is not a finite scalar".into(),
                origin: None,
                span: None,
                name_resolution: false,
                diagnostic_code: None,
                hint: None,
                quick_fix: None,
            })
        );
    }
}

// SPDX-License-Identifier: Apache-2.0

#[path = "../../../fslc/tests/support/edition_dependency_cases.rs"]
mod cases;
#[path = "../../../fslc/tests/support/edition_cases.rs"]
mod common;
use super::*;
use fslc_rust::{check_stages::run_check_stages, migration::admit_source};
use serde_json::{Map, Value, json};

fn state(root: &Url, child: &Url, source: &str, dependency: &str) -> ServerState {
    let document = |text: &str| OpenDocument {
        text: text.into(),
        version: 1,
    };
    ServerState {
        documents: HashMap::from([
            (root.clone(), document(source)),
            (child.clone(), document(dependency)),
        ]),
        parse_options: common::gated(),
        ..ServerState::default()
    }
}

fn range(source: &str, start: usize, end: usize) -> Value {
    let point = |offset| {
        let (line, _, character) = cases::position(source, offset);
        json!({"line":line,"character":character})
    };
    json!({"start":point(start),"end":point(end)})
}

fn published(state: &ServerState, uri: &Url) -> Value {
    let (sender, receiver) = crossbeam_channel::unbounded();
    publish_diagnostics(&sender, state, uri).unwrap();
    let Message::Notification(notification) = receiver.recv().unwrap() else {
        panic!("publish notification")
    };
    notification.params
}

#[allow(clippy::too_many_lines)] // One table-driven Store/presentation adapter.
fn dependency_edition_lsp_both_store_routes_use_open_child_and_child_utf16() {
    let scratch = common::Scratch::new();
    let root_uri = Url::from_file_path(scratch.0.join("root.fsl")).unwrap();
    let child_uri = Url::from_file_path(scratch.0.join("dep.fsl")).unwrap();
    let mut observations = cases::Observations::default();
    for (id, root_header, child_source, disk_source, severity) in [
        ("L01", "", cases::next_leaf(), cases::LEAF.to_owned(), 1),
        ("L02", "", cases::LEAF.to_owned(), cases::next_leaf(), 0),
        (
            "L05/untitled",
            "",
            cases::LEGACY_LEAF.into(),
            cases::LEGACY_LEAF.into(),
            2,
        ),
        (
            "L05",
            "fsl 5\n",
            cases::LEGACY_LEAF.replace(
                "action finish()",
                "@requirement(\"REQ-HEAD-001\", \"😀\") action finish()",
            ),
            cases::LEAF.to_owned(),
            2,
        ),
    ] {
        let untitled = id.ends_with("untitled");
        let root_uri = if untitled {
            Url::parse("untitled:Unsaved/Untitled-1").unwrap()
        } else {
            root_uri.clone()
        };
        assert!(!untitled || root_uri.to_file_path().is_err());
        let reference = if untitled {
            format!(
                "../../{}/dep.fsl",
                scratch
                    .0
                    .strip_prefix(common::root())
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/")
            )
        } else {
            "dep.fsl".into()
        };
        let source = format!(
            "{root_header}compose C {{\n  use S as s from \"{reference}\"\n  use S as q from \"./{reference}\"\n}}\n"
        );
        scratch.write("dep.fsl", &disk_source);
        let state = state(&root_uri, &child_uri, &source, &child_source);
        let resolver = StoreResolver {
            state: &state,
            base: if untitled {
                ".".into()
            } else {
                scratch.0.clone()
            },
        };
        let (_, model) = fslc_rust::source_diagnostic::diagnostics_with_model(
            &source,
            &root_uri
                .to_file_path()
                .map_or_else(|()| root_uri.to_string(), |p| p.display().to_string()),
            &resolver,
            &state.parse_options,
        );
        let actual = published(&state, &root_uri);
        let diagnostics = if severity == 0 {
            vec![]
        } else {
            let finding = cases::metadata_finding(
                &child_source,
                child_uri.to_file_path().unwrap().to_str().unwrap(),
                if severity == 1 { "next" } else { "current" },
            );
            let (start, end) = cases::metadata_span(&child_source);
            let reference = source.find("use").unwrap();
            vec![
                json!({"range":range(&source, reference, reference+3),"severity":severity,"code":finding["code"],"source":"fslc","message":finding["message"],"data":{"kind":"migration"},
                "relatedInformation":[{"location":{"uri":child_uri,"range":range(&child_source,start,end)},"message":finding["message"]}]}),
            ]
        };
        let expected = json!({"uri":root_uri,"version":1,"diagnostics":diagnostics});
        observations.eq(&format!("{id}/model"), model.is_some(), severity != 1);
        observations.eq(&format!("{id}/publish"), &actual, &expected);
    }
    let quick_source =
        "compose C { use S as s from \"dep.fsl\" invariant Root { forall i in 0..1: true } }\n";
    scratch.write("dep.fsl", &cases::next_leaf());
    let quick_state = state(&root_uri, &child_uri, quick_source, cases::LEAF);
    let colon = quick_source.find(':').unwrap();
    let end = quick_source.find("true").unwrap() + 4;
    let expected_actions = json!([{"title":"Use canonical FSL: braced quantifier","kind":"quickfix","edit":{"changes":{root_uri.as_str():[
        {"range":range(quick_source,colon,colon+1),"newText":" {"},{"range":range(quick_source,end,end),"newText":" }"}]}},"isPreferred":true}]);
    let actual_actions = serde_json::to_value(code_actions(&quick_state, &root_uri)).unwrap();
    observations.eq("L04/root-only-quickfix", &actual_actions, &expected_actions);
    scratch.write("dep.fsl", cases::LEAF);
    let refused_state = state(&root_uri, &child_uri, quick_source, &cases::next_leaf());
    let refused = serde_json::to_value(code_actions(&refused_state, &root_uri)).unwrap();
    observations.eq("L03/refusal-gate", refused, Value::Null);
    let source = "compose C {\n  use S as s from \"dep.fsl\"\n}\n";
    let dependency = format!("fsl 99\n{}", cases::LEAF);
    let state = state(&root_uri, &child_uri, source, &dependency);
    let actual = published(&state, &root_uri);
    let message = "unsupported syntax edition 'fsl 99'; supported editions: 4, 5";
    let expected = json!({"uri":root_uri,"version":1,"diagnostics":[{"range":range(source,source.find("use").unwrap(),source.find("use").unwrap()+3),"severity":1,"code":"unsupported_edition","source":"fslc","message":message,"data":{"kind":"parse"},"relatedInformation":[{"location":{"uri":child_uri,"range":range(&dependency,0,6)},"message":message}]}]});
    observations.eq("L06/publish", &actual, &expected);
    observations.eq(
        "L06/actions",
        code_actions(&state, &root_uri).is_none(),
        true,
    );
    observations.finish();
}

fn dependency_edition_t21_existing_requirements_solver_thread_is_preserved() {
    let scratch = common::Scratch::new();
    let uri = Url::from_file_path(scratch.0.join("requirements.fsl")).unwrap();
    let text = include_str!("../../../fslc/tests/fixtures/fsl5_solver/large_domain.fsl");
    let state = state(&uri, &uri, text, text);
    let mut replies = Vec::new();
    for _ in 0..2 {
        replies.push(published(&state, &uri));
    }
    assert_eq!(replies[0], replies[1]);
    let native = std::thread::spawn(move || {
        let options = common::gated();
        let admission =
            admit_source(text, "requirements.fsl", None, &options, &Map::new, false).unwrap();
        let mut solver = || fsl_solver_z3::Z3Solver::new().map_err(|e| e.to_string());
        match block_on_native(run_check_stages(
            text,
            "requirements.fsl",
            "requirements.fsl",
            &fsl_core::FsResolver::new("."),
            &Map::new,
            &options,
            &mut solver,
            &admission,
        )) {
            Err((output, 2)) => output,
            _ => panic!("native check must reject the decision table"),
        }
    })
    .join()
    .unwrap();
    assert_eq!(native["diagnostic_code"], "decide_not_exhaustive");
    let mut observations = cases::Observations::default();
    let expected = json!({"uri":uri,"version":1,"diagnostics":[{
        "range":{"start":{"line":26,"character":2},"end":{"line":26,"character":42}},
        "severity":1,"code":"decide_not_exhaustive","source":"fslc","data":{"kind":"semantics"},
        "message":native["message"]
    }]});
    observations.eq("T21", &replies[0], &expected);
    observations.finish();
}

#[test]
fn dependency_edition_lsp_adapter_executes_all_assigned_rows() {
    cases::exercise(
        "lsp",
        &[
            dependency_edition_lsp_both_store_routes_use_open_child_and_child_utf16,
            dependency_edition_t21_existing_requirements_solver_thread_is_preserved,
        ],
    );
}

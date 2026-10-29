// SPDX-License-Identifier: Apache-2.0

#[path = "../../../fslc/tests/support/edition_dependency_cases.rs"]
mod cases;
#[path = "../../../fslc/tests/support/edition_cases.rs"]
mod common;
use super::*;
use serde_json::{Value, json};

fn state(root: &Url, child: &Url, source: &str, dependency: &str) -> ServerState {
    ServerState {
        documents: HashMap::from([
            (
                root.clone(),
                OpenDocument {
                    text: source.into(),
                    version: 1,
                },
            ),
            (
                child.clone(),
                OpenDocument {
                    text: dependency.into(),
                    version: 1,
                },
            ),
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
        let source = format!("{root_header}compose C {{\n  use S as s from \"dep.fsl\"\n}}\n");
        scratch.write("dep.fsl", &disk_source);
        let state = state(&root_uri, &child_uri, &source, &child_source);
        let resolver = StoreResolver {
            state: &state,
            base: scratch.0.clone(),
        };
        let (_, model) = fslc_rust::source_diagnostic::diagnostics_with_model(
            &source,
            root_uri.to_file_path().unwrap().to_str().unwrap(),
            &resolver,
            &state.parse_options,
        );
        let (sender, receiver) = crossbeam_channel::unbounded();
        publish_diagnostics(&sender, &state, &root_uri).unwrap();
        let Message::Notification(notification) = receiver.recv().unwrap() else {
            panic!("publish notification");
        };
        let actual = notification.params;
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
        let labels: &[&str] = match id {
            "L01" => &["L01", "L03"],
            "L02" => &["L02", "L04"],
            _ => &["L05"],
        };
        for label in labels {
            observations.eq(&format!("{label}/model"), model.is_some(), severity != 1);
            observations.eq(&format!("{label}/publish"), &actual, &expected);
        }
        if matches!(id, "L01" | "L02") {
            let id = if id == "L01" { "L03" } else { "L04" };
            let actions = serde_json::to_value(code_actions(&state, &root_uri)).unwrap();
            observations.eq(&format!("{id}/actions"), &actions, &Value::Null);
        }
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
    let source = "compose C {\n  use S as s from \"dep.fsl\"\n}\n";
    let dependency = format!("fsl 99\n{}", cases::LEAF);
    let state = state(&root_uri, &child_uri, source, &dependency);
    let (sender, receiver) = crossbeam_channel::unbounded();
    publish_diagnostics(&sender, &state, &root_uri).unwrap();
    let Message::Notification(notification) = receiver.recv().unwrap() else {
        panic!("notification");
    };
    let message = "unsupported syntax edition 'fsl 99'; supported editions: 4, 5";
    let expected = json!({"uri":root_uri,"version":1,"diagnostics":[{"range":range(source,source.find("use").unwrap(),source.find("use").unwrap()+3),"severity":1,"code":"unsupported_edition","source":"fslc","message":message,"data":{"kind":"parse"},"relatedInformation":[{"location":{"uri":child_uri,"range":range(&dependency,0,6)},"message":message}]}]});
    observations.eq("L06/publish", &notification.params, &expected);
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
    let state = ServerState {
        documents: HashMap::from([(
            uri.clone(),
            OpenDocument {
                text: text.into(),
                version: 1,
            },
        )]),
        parse_options: common::gated(),
        ..ServerState::default()
    };
    let (sender, receiver) = crossbeam_channel::unbounded();
    let mut replies = Vec::new();
    for _ in 0..2 {
        publish_diagnostics(&sender, &state, &uri).unwrap();
        let Message::Notification(n) = receiver.recv().unwrap() else {
            panic!("publish");
        };
        replies.push(n.params);
    }
    assert_eq!(replies[0], replies[1]);
    assert_eq!(replies[0]["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(
        replies[0]["diagnostics"][0]["code"],
        "decide_not_exhaustive"
    );
    let mut observations = cases::Observations::default();
    observations.eq("T21", &replies[0], &replies[1]);
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

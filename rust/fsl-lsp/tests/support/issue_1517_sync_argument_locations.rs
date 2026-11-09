// SPDX-License-Identifier: Apache-2.0

#[path = "../../../fslc/tests/support/issue_1517_sync_argument_cases.rs"]
mod forms;
#[path = "../../../fslc/tests/support/issue_1517_scratch.rs"]
mod scratch;

use super::{Session, json, send};
use serde_json::Value;
use std::{fs, io::Read, path::PathBuf, time::Duration, time::Instant};

// Six measured families plus three independent range rules: parentheses,
// genuine conditional-arm precision, and an astral scalar before the actual.
fn fixture(id: &str) -> (forms::Case, (usize, usize, usize)) {
    let base = match id {
        "parenthesized_bool" | "inner_bool" | "utf16_bool" => "root_bool",
        _ => id,
    };
    let mut case = forms::cases().into_iter().find(|c| c.id == base).unwrap();
    let range = match id {
        "root_bool" => (4, 15, 19),
        "root_index" => (4, 15, 24),
        "second_actual_index" => (4, 18, 27),
        "root_pad_bool" => (7, 15, 19),
        "component_index" => (1, 2, 5),
        "valid_zero" => (0, 0, 0),
        "parenthesized_bool" => {
            case.source = forms::root("(true)");
            (4, 15, 21)
        }
        "inner_bool" => {
            case.source = forms::root("if true then 0 else true");
            case.error = Some((5, 36, "invalid model expression: expression of type Bool is not assignable to Named(\"part.K\") at 5:36".into()));
            (4, 35, 39)
        }
        "utf16_bool" => {
            case.source = case.source.replace(
                "  action step() =\n      ",
                "  action idle() \"😀\" {} action step() = ",
            );
            case.error = Some((4, 49, "invalid model expression: expression of type Bool is not assignable to Named(\"part.K\") at 4:49".into()));
            (3, 49, 53)
        }
        _ => panic!("unknown finite case {id}"),
    };
    (case, range)
}

fn publication(uri: &str, version: i32, diagnostics: &Value) -> Value {
    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
        "params":{"uri":uri,"version":version,"diagnostics":diagnostics}})
}

fn matching(session: &mut Session, uri: &str, version: i32, frames: &mut Vec<Value>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let message = session
            .stdout
            .recv(deadline)
            .expect("bounded LSP notification");
        frames.push(message.clone());
        if message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == uri
            && message["params"]["version"] == version
        {
            return message;
        }
    }
}

fn run(id: &str) {
    let (case, (line, start, end)) = fixture(id);
    assert_eq!(case.spec, "Root");
    let scratch = scratch::Scratch::new(
        "FSLC_1517_D_INPUTS",
        &format!("lsp/{id}"),
        &format!("lsp-{id}"),
    );
    let inputs = scratch.path();
    for (name, text) in [
        ("root.fsl", case.source.as_str()),
        ("piece.fsl", &case.piece),
        ("good.fsl", forms::GOOD),
    ] {
        fs::write(inputs.join(name), text).unwrap();
    }
    // A well-formed URI on every platform: `file://C:\...` is normalized by the
    // server, so the published URI would never equal it on Windows.
    let uri = lsp_types::Url::from_file_path(inputs.join("root.fsl"))
        .expect("absolute scratch path")
        .to_string();
    let mut diagnostics = case.error.as_ref().map_or_else(|| json!([]), |(_, _, message)| {
        json!([{"range":{"start":{"line":line,"character":start},"end":{"line":line,"character":end}},
            "severity":1,"code":"FSL-SEMANTIC","source":"fslc","message":message,"data":{"kind":"semantics"}}])
    });
    if id == "utf16_bool" {
        // The astral title is also legacy metadata: preserve its entire warning.
        diagnostics.as_array_mut().unwrap().push(json!({
            "range":{"start":{"line":3,"character":16},"end":{"line":3,"character":20}},
            "severity":2,"code":"legacy_string_metadata","source":"fslc",
            "message":"legacy string metadata is deprecated; use a typed annotation",
            "data":{"kind":"migration"}}));
    }
    // relatedInformation, tags and codeDescription are absent, not masked.
    let expected = publication(&uri, 1, &diagnostics);
    let clear_expected = publication(&uri, 2, &json!([]));
    let valid = "spec Good { state { seen: Bool } init { seen = false } }";
    let change = json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":valid}]}});
    let mut session = Session::start();
    let mut stderr = session.child.stderr.take().unwrap();
    session.open(&uri, &case.source);
    let mut frames = Vec::new();
    let actual = matching(&mut session, &uri, 1, &mut frames);
    send(&mut session.stdin, &change);
    let clear = matching(&mut session, &uri, 2, &mut frames);
    session.finish(); // Always finish before testing desired semantics.
    let mut errors = Vec::new();
    stderr.read_to_end(&mut errors).unwrap();
    if let Some(receipts) = std::env::var_os("FSLC_1517_D_RECEIPTS") {
        let dest = PathBuf::from(receipts).join("lsp").join(id);
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("stderr.raw"), &errors).unwrap();
        fs::write(dest.join("packets.json"), serde_json::to_vec_pretty(&json!({
            "id":id,"uri":uri,"source":case.source,"piece":case.piece,"change":change,"received":frames,
            "actual":actual,"expected":expected,"clear":clear,"clear_expected":clear_expected,
            "diagnostic_pass":actual==expected,"clear_pass":clear==clear_expected})).unwrap()).unwrap();
    }
    assert!(
        errors.is_empty() && actual == expected && clear == clear_expected,
        "{id}: actual={actual}; expected={expected}; clear={clear}; clear_expected={clear_expected}; stderr={errors:?}"
    );
}

macro_rules! cases {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { run(stringify!($name)); })+ };
}
cases!(
    root_bool,
    root_index,
    second_actual_index,
    root_pad_bool,
    component_index,
    valid_zero,
    parenthesized_bool,
    inner_bool,
    utf16_bool
);

// SPDX-License-Identifier: Apache-2.0

//! #1351 decision A reaches editor diagnostics through the native TSG builder.
//! A true orphan cannot be authored through this model-only LSP projection:
//! empty requirement blocks emit no requirement node, while linked Kernel
//! targets are all anchors under decision A. The CLI graph-API test preserves
//! genuine-orphan detection without inventing new LSP projection behavior.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

fn send(stdin: &mut ChildStdin, value: &Value) {
    let body = serde_json::to_vec(value).expect("serialize LSP message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
    stdin.write_all(&body).expect("write body");
    stdin.flush().expect("flush message");
}

fn receive(stdout: &mut BufReader<ChildStdout>) -> Value {
    let mut length = None;
    loop {
        let mut header = String::new();
        assert!(
            stdout.read_line(&mut header).expect("read header") > 0,
            "LSP closed output"
        );
        if header == "\r\n" {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length: ") {
            length = Some(value.trim().parse::<usize>().expect("content length"));
        }
    }
    let mut body = vec![0; length.expect("content-length header")];
    stdout.read_exact(&mut body).expect("read body");
    serde_json::from_slice(&body).expect("LSP JSON")
}

fn diagnostics(analysis_enabled: bool) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc-lsp"))
        .env(
            "FSLC_LSP_ANALYSIS_DIAGNOSTICS",
            if analysis_enabled { "1" } else { "0" },
        )
        .env_remove("FSLC_UNSTABLE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn native LSP");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0", "id":1, "method":"initialize",
            "params":{"processId":null,"rootUri":null,"capabilities":{}}
        }),
    );
    assert_eq!(receive(&mut stdout)["id"], 1);
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    );
    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0", "method":"textDocument/didOpen",
            "params":{"textDocument":{
                "uri":"file:///fsl-issue-1351-init.fsl", "languageId":"fsl", "version":1,
                "text":"spec InitDiagnostics {\n  state { ready: Bool }\n  @requirement(\"MODEL-INIT-001\", \"Model startup value\")\n  init { ready = false }\n}\n"
            }}
        }),
    );
    let published = receive(&mut stdout);
    assert_eq!(published["method"], "textDocument/publishDiagnostics");
    assert_eq!(published["params"]["version"], 1);
    let diagnostics = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .clone();
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
    );
    assert_eq!(receive(&mut stdout)["id"], 2);
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"exit","params":null}),
    );
    drop(stdin);
    assert!(child.wait().expect("wait for LSP").success());
    diagnostics
}

#[test]
fn init_only_requirement_has_no_disconnected_requirement_editor_diagnostic() {
    let published = diagnostics(true);
    // Live control: a disabled analysis channel must not make this test green.
    let unwritten = published
        .iter()
        .find(|item| item["code"] == "unwritten_state")
        .unwrap_or_else(|| panic!("analysis diagnostics were not enabled: {published:#?}"));
    assert_eq!(unwritten["source"], "fslc analyze");
    assert_eq!(unwritten["severity"], 3);
    assert_eq!(unwritten["data"]["formal_status"], "not_a_violation");
    assert!(
        !published.iter().any(|item| item["severity"] == 1),
        "{published:#?}"
    );
    assert!(
        !published
            .iter()
            .any(|item| item["code"] == "disconnected_requirement"),
        "init-only requirement still produces an editor false positive: {published:#?}"
    );
}

#[test]
fn disabled_analysis_diagnostics_do_not_publish_structural_findings() {
    let published = diagnostics(false);
    assert!(
        published
            .iter()
            .all(|item| item["source"] != "fslc analyze"),
        "{published:#?}"
    );
    assert!(
        !published.iter().any(|item| item["severity"] == 1),
        "{published:#?}"
    );
}

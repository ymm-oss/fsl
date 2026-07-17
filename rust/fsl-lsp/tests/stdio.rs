// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

fn send(stdin: &mut ChildStdin, message: &Value) {
    let body = serde_json::to_vec(message).expect("serialize LSP message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write LSP header");
    stdin.write_all(&body).expect("write LSP body");
    stdin.flush().expect("flush LSP message");
}

fn receive(stdout: &mut BufReader<ChildStdout>) -> Value {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        stdout.read_line(&mut header).expect("read LSP header");
        if header == "\r\n" {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length: ") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .expect("numeric content length"),
            );
        }
    }
    let mut body = vec![0_u8; content_length.expect("content length header")];
    stdout.read_exact(&mut body).expect("read LSP body");
    serde_json::from_slice(&body).expect("parse LSP body")
}

#[test]
fn stdio_lifecycle_uses_latest_buffer_and_serves_symbols() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fslc-lsp");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    let uri = "file:///tmp/fsl-lsp-stdio.fsl";

    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"processId":null,"rootUri":null,"capabilities":{}}
        }),
    );
    let initialized = receive(&mut stdout);
    assert_eq!(initialized["id"], 1);
    assert_eq!(
        initialized["result"]["capabilities"]["renameProvider"],
        true
    );
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    );

    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
                "uri":uri,"languageId":"fsl","version":1,
                "text":"spec Broken { state { value: Missing } init { value = 0 } }"
            }}
        }),
    );
    let invalid = receive(&mut stdout);
    assert_eq!(invalid["method"], "textDocument/publishDiagnostics");
    assert_eq!(invalid["params"]["version"], 1);
    assert_eq!(invalid["params"]["diagnostics"][0]["data"]["kind"], "type");

    let valid = "spec Fixed { state { ready: Bool } init { ready = false } }";
    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didChange","params":{
                "textDocument":{"uri":uri,"version":2},
                "contentChanges":[{"text":valid}]
            }
        }),
    );
    let changed = receive(&mut stdout);
    assert_eq!(changed["params"]["version"], 2);
    assert_eq!(changed["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":2,"method":"textDocument/documentSymbol",
            "params":{"textDocument":{"uri":uri}}
        }),
    );
    let symbols = receive(&mut stdout);
    assert_eq!(symbols["id"], 2);
    assert!(symbols["result"].as_array().is_some_and(|items| {
        items.iter().any(|item| item["name"] == "Fixed")
            && items.iter().any(|item| item["name"] == "ready")
    }));

    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didSave","params":{
                "textDocument":{"uri":uri},
                "text":"spec Saved { state { value: Missing } init { value = 0 } }"
            }
        }),
    );
    let saved = receive(&mut stdout);
    assert_eq!(saved["params"]["version"], 2);
    assert_eq!(saved["params"]["diagnostics"][0]["data"]["kind"], "type");

    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didClose","params":{
                "textDocument":{"uri":uri}
            }
        }),
    );
    let closed = receive(&mut stdout);
    assert_eq!(closed["params"]["diagnostics"], json!([]));

    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
    );
    assert_eq!(receive(&mut stdout)["id"], 3);
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"exit","params":null}),
    );
    drop(stdin);
    assert!(child.wait().expect("wait for fslc-lsp").success());
}

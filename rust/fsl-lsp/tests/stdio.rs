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
        let read = stdout.read_line(&mut header).expect("read LSP header");
        assert!(
            read > 0,
            "fslc-lsp closed its output (did the server panic?)"
        );
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

/// A diagnostic whose fallback span sits after non-ASCII text on its line
/// (an unlocated `undefined predicate` after a Japanese / emoji title) used
/// to become a byte offset inside a character, and the server panicked
/// slicing the source to convert it. It must publish the diagnostic at the
/// token, in UTF-16 code units (the emoji counts twice).
#[test]
fn diagnostic_after_non_ascii_text_is_published_at_its_utf16_position() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fslc-lsp");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"processId":null,"rootUri":null,"capabilities":{}}
        }),
    );
    assert_eq!(receive(&mut stdout)["id"], 1);
    send(
        &mut stdin,
        &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
    );
    let text = "spec Probe {\n  state { x: Int }\n  init { x = 0 }\n  invariant I \"不変😀\" { undefinedPred(x) }\n}\n";
    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
                "uri":"file:///tmp/fsl-lsp-utf16.fsl","languageId":"fsl","version":1,"text":text
            }}
        }),
    );
    let published = receive(&mut stdout);
    assert_eq!(published["method"], "textDocument/publishDiagnostics");
    let diagnostic = &published["params"]["diagnostics"][0];
    assert!(
        diagnostic["message"]
            .as_str()
            .is_some_and(|message| message.contains("undefined predicate 'undefinedPred'")),
        "{published:#}"
    );
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line":3,"character":23}),
        "{published:#}"
    );
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
    assert!(child.wait().expect("wait for fslc-lsp").success());
}

/// Open `text` in a fresh server with the unstable gate set or unset and
/// return the diagnostics the server publishes for it.
fn published_diagnostics(text: &str, unstable: Option<&str>) -> Vec<Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc-lsp"));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("FSLC_UNSTABLE");
    if let Some(features) = unstable {
        command.env("FSLC_UNSTABLE", features);
    }
    let mut child = command.spawn().expect("spawn fslc-lsp");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    send(
        &mut stdin,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
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
            "jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
                "uri":"file:///tmp/fsl-lsp-astral.fsl","languageId":"fsl","version":1,"text":text
            }}
        }),
    );
    let published = receive(&mut stdout);
    assert_eq!(published["method"], "textDocument/publishDiagnostics");
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
    assert!(child.wait().expect("wait for fslc-lsp").success());
    published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .clone()
}

/// fsl 5 plan S2 acceptance 4: on a line holding a character outside the
/// BMP (CJK Extension B `𠮷`, two UTF-16 code units), every published range
/// is in UTF-16 code units, including the lexer's own warnings behind the
/// gate. Without the gate the same document publishes no lexical warning.
#[test]
fn ranges_after_a_character_outside_the_bmp_count_utf16_units() {
    let text = "fsl 5\nspec S {\n  state { 𠮷野\u{3000}: Int }\n}\n";
    let range = |line: u32, start: u32, end: u32| json!({"start":{"line":line,"character":start},"end":{"line":line,"character":end}});

    let gated = published_diagnostics(text, Some("fsl5"));
    let warning = gated
        .iter()
        .find(|diagnostic| diagnostic["code"] == "fullwidth_space")
        .unwrap_or_else(|| panic!("fullwidth_space published: {gated:#?}"));
    assert_eq!(warning["severity"], 2, "{warning:#}");
    assert_eq!(warning["range"], range(2, 13, 14), "{warning:#}");
    assert_eq!(warning["data"]["replacement"], " ");

    // With the gate the header selects the wider identifier rule for every
    // parse, so the document is valid; without it `𠮷` is a lexer error.
    assert!(
        gated.iter().all(|diagnostic| diagnostic["severity"] != 1),
        "{gated:#?}"
    );
    let error_at = |text: &str, unstable: Option<&str>| {
        let diagnostics = published_diagnostics(text, unstable);
        diagnostics
            .iter()
            .find(|diagnostic| diagnostic["severity"] == 1)
            .unwrap_or_else(|| panic!("an error is published: {diagnostics:#?}"))["range"]
            .clone()
    };
    assert_eq!(error_at(text, None), range(2, 10, 12));
    let unknown_type = "fsl 5\nspec S {\n  state { 𠮷野\u{3000}: Nope }\n}\n";
    // The field declaration: it ends after `Nope` at 20 UTF-16 units (19 chars).
    assert_eq!(error_at(unknown_type, Some("fsl5")), range(2, 10, 20));
    assert!(
        published_diagnostics(text, None)
            .iter()
            .all(|diagnostic| diagnostic["data"]["kind"] != "lexical")
    );
}

/// fsl 5 plan S1 acceptance 5: an `fsl 5` document whose first line is a
/// Japanese `//` comment is served without a panic, and its migration
/// diagnostics are errors, as `--edition next` makes them. The same body
/// without the header, or without the gate, keeps them warnings.
#[test]
fn an_fsl_5_header_after_a_japanese_comment_makes_migration_diagnostics_errors() {
    const ERROR: u64 = 1;
    const WARNING: u64 = 2;
    let body = "spec Quantifier {\n  type Id = 0..1\n  state { ready: Map<Id, Bool> }\n  init { forall i in 0..1: { ready[i] = false } }\n  invariant Typed { forall i in 0..1: ready[i] == true or ready[i] == false }\n}\n";
    let migration = |diagnostics: &[Value]| {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["data"]["kind"] == "migration")
            .map(|diagnostic| {
                (
                    diagnostic["code"].as_str().unwrap_or_default().to_owned(),
                    diagnostic["severity"].as_u64().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
    };

    let headed = format!("// 日本語のコメント\nfsl 5\n{body}");
    let gated = published_diagnostics(&headed, Some("fsl5"));
    let found = migration(&gated);
    assert!(!found.is_empty(), "{gated:#?}");
    assert!(
        found.iter().all(|(_, severity)| *severity == ERROR),
        "{gated:#?}"
    );
    assert!(
        gated
            .iter()
            .all(|diagnostic| diagnostic["code"] != "FSL-DIALECT-UNKNOWN"),
        "{gated:#?}"
    );

    let plain = migration(&published_diagnostics(
        &format!("// 日本語のコメント\n{body}"),
        Some("fsl5"),
    ));
    assert_eq!(
        plain
            .iter()
            .map(|(code, _)| code.as_str())
            .collect::<Vec<_>>(),
        found
            .iter()
            .map(|(code, _)| code.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        plain.iter().all(|(_, severity)| *severity == WARNING),
        "{plain:?}"
    );

    let ungated = published_diagnostics(&headed, None);
    assert!(
        ungated
            .iter()
            .any(|diagnostic| diagnostic["code"] == "FSL-DIALECT-UNKNOWN"),
        "without the gate the header is the unknown declaration `fsl`: {ungated:#?}"
    );
}

/// #1243: the editor published no error for a forbidden the requirement-trace
/// gate rejects, although `check` refuses the spec. It now publishes the gate's
/// error where `check` locates it (`loc` line 19, column 3 of the gallery
/// fixture: the `forbidden FB-1` case), and the server keeps serving
/// (`published_diagnostics` shuts it down cleanly). With the guard restored,
/// the gate passes and nothing is published.
#[test]
fn a_forbidden_the_gate_rejects_is_published_as_an_error() {
    const ERROR: u64 = 1;
    let fixture = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/gallery/errors/forbidden_op_accepted.fsl"),
    )
    .expect("read gallery fixture");
    let published = published_diagnostics(&fixture, None);
    let trace = published
        .iter()
        .filter(|diagnostic| diagnostic["code"] == "FSL-REQUIREMENT-TRACE")
        .collect::<Vec<_>>();
    assert_eq!(trace.len(), 1, "{published:#?}");
    assert_eq!(trace[0]["severity"], ERROR);
    assert_eq!(trace[0]["data"]["kind"], "forbidden");
    assert_eq!(
        trace[0]["range"]["start"],
        json!({"line": 18, "character": 2})
    );
    let message = trace[0]["message"].as_str().expect("message");
    assert!(
        message.starts_with("forbidden 'FB-1' does not pass the requirement-trace gate: "),
        "{message}"
    );

    let guarded = fixture.replace(
        "requires order[o] == Paid or order[o] == Shipped",
        "requires order[o] == Paid",
    );
    assert_ne!(guarded, fixture, "the fixture's weakened guard moved");
    let published = published_diagnostics(&guarded, None);
    assert!(
        published
            .iter()
            .all(|diagnostic| diagnostic["code"] != "FSL-REQUIREMENT-TRACE"),
        "{published:#?}"
    );
}

/// One `fslc-lsp` process driven request by request; notifications the
/// server publishes in between (diagnostics) are skipped.
struct Session {
    child: std::process::Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fslc-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("FSLC_UNSTABLE")
            .spawn()
            .expect("spawn fslc-lsp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        let mut session = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        session.request(
            "initialize",
            &json!({"processId":null,"rootUri":null,"capabilities":{}}),
        );
        send(
            &mut session.stdin,
            &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        );
        session
    }

    fn open(&mut self, uri: &str, text: &str) {
        send(
            &mut self.stdin,
            &json!({
                "jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
                    "uri":uri,"languageId":"fsl","version":1,"text":text
                }}
            }),
        );
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        send(
            &mut self.stdin,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        );
        loop {
            let message = receive(&mut self.stdout);
            if message["id"] == id {
                return message["result"].clone();
            }
        }
    }

    fn finish(mut self) {
        self.request("shutdown", &Value::Null);
        send(
            &mut self.stdin,
            &json!({"jsonrpc":"2.0","method":"exit","params":null}),
        );
        drop(self.stdin);
        assert!(self.child.wait().expect("wait for fslc-lsp").success());
    }
}

/// Issue 1362 over the protocol: a state variable (`rejected` in
/// `specs/rate_limiter.fsl`), a struct field (`status` in
/// `specs/order_workflow.fsl`) and an action used as an acceptance step
/// (`respond(0)` in `examples/nfr/support_sla.fsl`) are spelled like index
/// keywords. Each one's use navigates to its declaration, its references are
/// every occurrence of the name, rename rewrites exactly those, and hover
/// names the declaration's role. The documents are opened under a directory
/// that does not exist, so no sibling file joins the workspace.
#[test]
fn keyword_spelled_names_navigate_rename_and_hover_like_any_other_name() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repository root");
    // (corpus file, name, a use as (line, character), its declaration, role,
    // occurrences of the name including the declaration), positions 0-based.
    let cases = [
        (
            "specs/rate_limiter.fsl",
            "rejected",
            (28, 15),
            (11, 4),
            "variable",
            6,
        ),
        (
            "specs/order_workflow.fsl",
            "status",
            (23, 14),
            (6, 17),
            "property",
            16,
        ),
        (
            "examples/nfr/support_sla.fsl",
            "respond",
            (77, 4),
            (37, 16),
            "function",
            2,
        ),
    ];
    let mut session = Session::start();
    for (file, name, (use_line, use_character), (line, character), role, occurrences) in cases {
        let text = std::fs::read_to_string(root.join(file)).expect("read corpus file");
        let uri = format!("file:///nonexistent-fsl-lsp-1362/{file}");
        session.open(&uri, &text);
        let at_use = json!({
            "textDocument":{"uri":uri},
            "position":{"line":use_line,"character":use_character}
        });
        let declaration = json!({
            "start":{"line":line,"character":character},
            "end":{"line":line,"character":character + name.len()}
        });

        let definition = session.request("textDocument/definition", &at_use);
        assert_eq!(definition["uri"], uri, "{file} {name}: {definition:#}");
        assert_eq!(definition["range"], declaration, "{file} {name}");

        let mut with_context = at_use.clone();
        with_context["context"] = json!({"includeDeclaration":true});
        let references = session.request("textDocument/references", &with_context);
        let references = references.as_array().expect("references array");
        assert_eq!(
            references.len(),
            occurrences,
            "{file} {name}: {references:#?}"
        );
        let lines = text.lines().collect::<Vec<_>>();
        for reference in references {
            let start = &reference["range"]["start"];
            let line =
                lines[usize::try_from(start["line"].as_u64().expect("line")).expect("usize")];
            let character =
                usize::try_from(start["character"].as_u64().expect("character")).expect("usize");
            assert_eq!(
                &line[character..character + name.len()],
                name,
                "{file}: {reference}"
            );
        }

        let mut rename = at_use.clone();
        rename["newName"] = json!("renamed");
        let edit = session.request("textDocument/rename", &rename);
        let edits = edit["changes"][&uri].as_array().expect("rename edits");
        assert_eq!(edits.len(), occurrences, "{file} {name}: {edit:#}");
        assert!(edits.iter().all(|edit| edit["newText"] == "renamed"));

        let hover = session.request("textDocument/hover", &at_use);
        assert!(
            hover["contents"]["value"]
                .as_str()
                .is_some_and(|value| value.ends_with(&format!("**{role}** `{name}`"))),
            "{file} {name}: {hover:#}"
        );
    }
    session.finish();
}

/// fsl 5 plan S7, decisions/048 J3: the language server, which has no
/// solver, publishes a decision table's gap with the values native `check`
/// reports, because fsl-core enumerates the field's domain.
#[test]
fn a_decision_table_gap_is_published_with_its_values() {
    let text = "fsl 5\nrequirements R {\n  number Amount\n  const AUTO_LIMIT = 1\n  \
                actor System, Manager\n  entity Claim {\n    amount: Amount = 0\n    \
                lifecycle Submitted -> Approved\n  }\n  \
                concept SmallClaim = Claim where amount < AUTO_LIMIT\n  \
                concept LargeClaim = Claim where amount > AUTO_LIMIT\n  \
                event auto_approve = System approves Claim Submitted -> Approved\n  \
                event mgr_approve = Manager approves Claim Submitted -> Approved\n  \
                decide T \"who\" for Submitted Claim {\n    \
                SmallClaim => System approves\n    LargeClaim => Manager approves\n  \
                }\n}\nverify {\n  instances Claim = 1\n  values Amount = 0..3\n}\n";
    let diagnostics = published_diagnostics(text, Some("fsl5"));
    let gap = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == "decide_not_exhaustive")
        .unwrap_or_else(|| panic!("decide_not_exhaustive published: {diagnostics:#?}"));
    assert_eq!(gap["severity"], 1, "{gap:#}");
    assert!(
        gap["message"]
            .as_str()
            .is_some_and(|message| message.contains("no row holds at amount = 1, AUTO_LIMIT = 1")),
        "{gap:#}"
    );
}

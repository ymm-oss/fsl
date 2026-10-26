// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

#[allow(dead_code)] // CLI/Worker also consume this inventory's engine metadata.
#[path = "../../fslc/tests/support/issue_1463_forms.rs"]
mod issue_1463_forms;

#[test]
fn issue_1463_supported_forms_publish_and_clear_located_diagnostics() {
    let cases = issue_1463_forms::cases();
    // Each invalid didOpen/didChange is followed by a valid document, so stale
    // errors cannot masquerade as the diagnostic for the next fixture.
    let valid =
        "spec Good { state { done: Bool } init { done = false } action go() { done = true } }";
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../target/issue-1463-lsp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("worktree-local resolver fixture");
    let mut texts = Vec::new();
    for c in &cases {
        for (name, source) in &c.files {
            std::fs::write(dir.join(name), source).expect("resolved component");
        }
        texts.push(c.source.as_str());
        texts.push(valid);
    }
    let uri = format!("file://{}", dir.join("support.fsl").display());
    let published = published_diagnostics_each_at(&texts, None, &uri);
    let (pairs, remainder) = published.as_chunks::<2>();
    assert!(remainder.is_empty());
    assert_eq!(pairs.len(), cases.len());
    let mut failures = Vec::new();
    for (c, pair) in cases.iter().zip(pairs) {
        let errors: Vec<_> = pair[0].iter().filter(|d| d["severity"] == 1).collect();
        assert!(
            pair[1].iter().all(|d| d["severity"] != 1),
            "{}: didChange failed to clear errors: {:?}",
            c.id,
            pair[1]
        );
        if c.rejection.is_some() {
            if errors.len() != 1 {
                failures.push(format!(
                    "{}: expected one source rejection; produced {:?}",
                    c.id, pair[0]
                ));
                continue;
            }
            let diagnostic = errors[0];
            let (line, column) = issue_1463_forms::utf16_location(c);
            assert_eq!(
                diagnostic["data"]["kind"], "semantics",
                "{}: {diagnostic}",
                c.id
            );
            assert_eq!(diagnostic["code"], "FSL-SEMANTIC", "{}: {diagnostic}", c.id);
            assert_eq!(
                diagnostic["range"]["start"],
                json!({"line":line,"character":column}),
                "{}: {diagnostic}",
                c.id
            );
            assert!(
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|m| !m.is_empty())
            );
        } else if c.expected[0] == issue_1463_forms::Outcome::Ok {
            assert!(
                errors.is_empty(),
                "{}: accepted source was rejected: {errors:?}",
                c.id
            );
        } else {
            assert!(
                !errors.is_empty(),
                "{}: existing rejection disappeared",
                c.id
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing LSP cells:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

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
    published_diagnostics_each(&[text], unstable)
        .pop()
        .expect("one publish")
}

#[test]
fn legacy_expression_operator_diagnostics_use_the_operator_utf16_range() {
    for (operator, replacement) in [("||", "or"), ("->", "=>")] {
        let line = format!(
            "  @requirement(\"REQ-PROBE-001\", \"日本語😀\") invariant I {{ true {operator} false }}"
        );
        let source = format!("fsl 5\nspec S {{\n  state {{ flag: Bool = false }}\n{line}\n}}\n");
        let diagnostics = published_diagnostics(&source, Some("fsl5"));
        let diagnostic = diagnostics
            .iter()
            .find(|d| d["code"] == "unsupported_in_edition")
            .unwrap_or_else(|| panic!("expected edition diagnostic, produced {diagnostics:#?}"));
        let column = line[..line.find(operator).expect("operator")]
            .encode_utf16()
            .count();
        assert_eq!(
            diagnostic["range"],
            json!({"start":{"line":3,"character":column},"end":{"line":3,"character":column+2}})
        );
        assert_eq!(diagnostic["severity"], 1);
        assert!(
            diagnostic["message"]
                .as_str()
                .is_some_and(|m| m.contains(&format!("use '{replacement}'"))
                    && m.contains("fslc migrate <file> --edition next"))
        );
        let canonical = source.replace(operator, replacement);
        assert!(
            published_diagnostics(&canonical, Some("fsl5"))
                .iter()
                .all(|d| d["code"] != "unsupported_in_edition")
        );
    }
}

#[test]
fn shared_expression_entries_publish_both_operator_endpoints() {
    for body in [
        "requirements R { state { flag: Bool = false } invariant I { true OP false } }",
        "compose C { state { flag: Bool = false } invariant I { true OP false } }",
        "business B { actor User entity Case process Case { stages New, Done initial New transition finish New -> Done by User } policy POL-PROBE-001 \"policy\" invariant { true OP false } } verify { instances Case = 1 }",
        "refinement R { impl I abs A map flag = true OP false }",
    ] {
        let column = body[..body.find("OP").expect("operator marker")]
            .encode_utf16()
            .count();
        for operator in ["||", "->"] {
            let source = format!("fsl 5\n{}\n", body.replace("OP", operator));
            let diagnostics = published_diagnostics(&source, Some("fsl5"));
            let diagnostic = diagnostics
                .iter()
                .find(|d| d["code"] == "unsupported_in_edition")
                .unwrap_or_else(|| panic!("expected operator diagnostic: {diagnostics:#?}"));
            assert_eq!(
                diagnostic["range"],
                json!({"start":{"line":1,"character":column},"end":{"line":1,"character":column+2}})
            );
        }
    }
}

/// Open the first of `texts` in a fresh server with the unstable gate set or
/// unset, replace the document's text with each of the others in turn, and
/// return the diagnostics the server publishes for each.
fn published_diagnostics_each(texts: &[&str], unstable: Option<&str>) -> Vec<Vec<Value>> {
    published_diagnostics_each_at(texts, unstable, "file:///tmp/fsl-lsp-astral.fsl")
}

fn published_diagnostics_each_at(
    texts: &[&str],
    unstable: Option<&str>,
    uri: &str,
) -> Vec<Vec<Value>> {
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
    let mut published = Vec::new();
    for (version, text) in texts.iter().enumerate() {
        if version == 0 {
            send(
                &mut stdin,
                &json!({
                    "jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
                        "uri":uri,"languageId":"fsl","version":1,"text":text
                    }}
                }),
            );
        } else {
            send(
                &mut stdin,
                &json!({
                    "jsonrpc":"2.0","method":"textDocument/didChange","params":{
                        "textDocument":{"uri":uri,"version":version + 1},
                        "contentChanges":[{"text":text}]
                    }
                }),
            );
        }
        let publish = receive(&mut stdout);
        assert_eq!(publish["method"], "textDocument/publishDiagnostics");
        published.push(
            publish["params"]["diagnostics"]
                .as_array()
                .expect("diagnostics array")
                .clone(),
        );
    }
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
    published
}

/// decisions/048 J3: a decision table the enumeration leaves to the solver
/// (a domain past the bound; a condition reading a resource) is published as
/// the error native `check` reports for it, code, location and message
/// alike, where the server used to publish none. The document is published
/// four times: native Z3 answers a question asked again in one context with
/// another counterexample by the third or fourth time, and no publish may
/// depend on the questions before it.
#[test]
fn requirements_the_solver_refutes_are_published_as_check_reports_them() {
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fslc/tests/fixtures/fsl5_solver");
    let options = fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    );
    for file in ["large_domain.fsl", "resource_condition.fsl"] {
        let text = std::fs::read_to_string(fixtures.join(file)).expect("read fixture");
        let resolver = fsl_core::FsResolver::new(&fixtures);
        // On a thread of its own, as a fresh `fslc check` process asks it.
        let native = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let mut z3 =
                        || fsl_solver_z3::Z3Solver::new().map_err(|error| error.to_string());
                    let mut future = std::pin::pin!(fslc_rust::check_stages::run_check_stages(
                        &text,
                        file,
                        file,
                        &resolver,
                        &serde_json::Map::new,
                        &options,
                        &mut z3,
                    ));
                    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                    match future.as_mut().poll(&mut context) {
                        std::task::Poll::Ready(Err((output, 2))) => output,
                        other => panic!("native check must reject {file}: {other:?}"),
                    }
                })
                .join()
                .expect("native check thread")
        });
        assert_eq!(
            native["diagnostic_code"], "decide_not_exhaustive",
            "{native:#}"
        );
        for diagnostics in published_diagnostics_each(&[text.as_str(); 4], Some("fsl5")) {
            let error = diagnostics
                .iter()
                .find(|diagnostic| diagnostic["severity"] == 1)
                .unwrap_or_else(|| panic!("{file}: an error is published: {diagnostics:#?}"));
            assert_eq!(
                (
                    &error["code"],
                    &error["message"],
                    error["range"]["start"]["line"]
                        .as_u64()
                        .map(|line| line + 1),
                    error["range"]["start"]["character"]
                        .as_u64()
                        .map(|column| column + 1),
                ),
                (
                    &native["diagnostic_code"],
                    &native["message"],
                    native["loc"]["line"].as_u64(),
                    native["loc"]["column"].as_u64(),
                ),
                "{file}: {error:#}"
            );
        }
    }
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

    let error_at = |text: &str, unstable: Option<&str>| {
        let diagnostics = published_diagnostics(text, unstable);
        diagnostics
            .iter()
            .find(|diagnostic| diagnostic["severity"] == 1)
            .unwrap_or_else(|| panic!("an error is published: {diagnostics:#?}"))
            .clone()
    };
    // With the gate the header selects the wider identifier rule, so the
    // lexer reads `𠮷野`; a spec has no glossary, so the name is refused as
    // not ASCII. Its range ends after `𠮷野` at 13 UTF-16 units (12 chars).
    let error = error_at(text, Some("fsl5"));
    assert_eq!(error["code"], "canonical_name_not_ascii", "{error:#}");
    assert_eq!(error["range"], range(2, 10, 13), "{error:#}");
    // Without the gate `𠮷` is a lexer error.
    assert_eq!(error_at(text, None)["range"], range(2, 10, 12));
    assert!(
        published_diagnostics(text, None)
            .iter()
            .all(|diagnostic| diagnostic["data"]["kind"] != "lexical")
    );
}

/// fsl 5 plan S11: in a requirements document written with Japanese
/// keywords and an alias holding a character outside the BMP, a diagnostic
/// after the alias on its line is published in UTF-16 code units (`𠮷` counts
/// twice), at the word the CLI's character column names.
#[test]
fn a_diagnostic_after_japanese_keywords_and_an_astral_alias_counts_utf16_units() {
    let text = "fsl 5\n要件 R {\n  用語集 {\n    Claim = 𠮷申請\n  }\n  \
                エンティティ 𠮷申請 { 金額: Int = 0  ライフサイクル A -> B }\n}\n";
    let diagnostics = published_diagnostics(text, Some("fsl5"));
    let error = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == "canonical_name_not_ascii")
        .unwrap_or_else(|| panic!("canonical_name_not_ascii published: {diagnostics:#?}"));
    assert_eq!(error["severity"], 1, "{error:#}");
    assert_eq!(
        error["range"],
        json!({"start":{"line":5,"character":16},"end":{"line":5,"character":18}}),
        "{error:#}"
    );
    // Without the gate the first Japanese keyword is not a word.
    let published = published_diagnostics(text, None);
    assert_eq!(
        published[0]["range"]["start"],
        json!({"line":1,"character":0}),
        "{published:#?}"
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

/// #1346: publish acceptance's wrong-kind message as an error at its step,
/// rather than the acceptance declaration or a generic not-enabled failure.
#[test]
fn an_acceptance_wrong_kind_step_is_published_as_a_located_error() {
    let source = r#"requirements Shop {
  enum Color { Red, Blue }
  state { color: Color }
  init { color = Red }
  requirement REQ-1 "paint" {
    action paint(c: Color) { requires c != color  color = c }
  }
  acceptance AC-1 "paint blue" {
    paint(1)
    expect color == Blue
  }
}
"#;
    let published = published_diagnostics(source, None);
    let trace = published
        .iter()
        .filter(|diagnostic| diagnostic["code"] == "FSL-REQUIREMENT-TRACE")
        .collect::<Vec<_>>();
    assert_eq!(trace.len(), 1, "{published:#?}");
    assert_eq!(trace[0]["severity"], 1, "{published:#?}");
    assert_eq!(trace[0]["data"]["kind"], "acceptance", "{published:#?}");
    assert_eq!(
        trace[0]["range"]["start"],
        json!({"line": 8, "character": 4}),
        "{published:#?}"
    );
    assert_eq!(
        trace[0]["message"],
        "acceptance 'AC-1' does not pass the requirement-trace gate: argument 1 for 'c' of 'paint' is not a value of type 'Color' in acceptance, so no guard was evaluated",
        "{published:#?}"
    );
}

/// #1439: both spellings publish a located semantic error on the authored
/// keyword token, not an acceptance replay error or the trace declaration.
#[test]
fn ill_typed_expect_and_then_publish_semantic_keyword_ranges() {
    for (header, trace, keyword, unstable, line, end) in [
        ("", "acceptance", "expect", None, 9, 10),
        ("fsl 5\n", "scenario", "then", Some("fsl5"), 10, 8),
    ] {
        let source = format!(
            "{header}requirements E {{\n  enum Color {{ Red, Blue }}\n  state {{ c: Color }}\n  init {{ c = Red }}\n  requirement REQ-1 \"paint\" {{\n    action paint(x: Color) {{ c = x }}\n  }}\n  {trace} AC-1 \"type comparison\" {{\n    paint(Blue)\n    {keyword} c != 1\n  }}\n}}\n"
        );
        let published = published_diagnostics(&source, unstable);
        let errors = published
            .iter()
            .filter(|diagnostic| diagnostic["severity"] == 1)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{keyword}: {published:#?}");
        let error = errors[0];
        assert_eq!(error["code"], "FSL-SEMANTIC");
        assert_eq!(error["severity"], 1);
        assert_eq!(error["data"]["kind"], "semantics");
        assert_eq!(
            error["range"],
            json!({"start": {"line": line, "character": 4},
                   "end": {"line": line, "character": end}})
        );
        assert!(error["message"].as_str().is_some_and(|message| {
            message.contains("expression of type Int is not assignable to Named(\"Color\")")
        }));
    }
}

#[test]
fn ill_typed_trace_arguments_publish_semantic_step_token_ranges() {
    for (source, unstable, line, end, type_name) in [
        (
            "requirements E {\n  enum Color { Red, Blue }\n  state { c: Color }\n  init { c = Red }\n  requirement REQ-1 \"paint\" {\n    action paint(x: Color) { requires x != c c = x }\n  }\n  forbidden FB-1 \"internal type\" {\n    paint(if c != 1 then Red else Blue)\n    expect rejected\n  }\n}\n",
            None,
            8,
            9,
            "Color",
        ),
        (
            "fsl 5\nrequirements R {\n  number Amount\n  actor Employee\n  entity Claim { amount: Amount = 0 lifecycle Draft -> Submitted }\n  event submit = Employee submits Claim with amount Draft -> Submitted\n  scenario AC-1 \"internal type\" {\n    Employee submits claim1 with amount (if claim_stage[0] != 1 then 1 else 2)\n    then claim1 is Submitted\n  }\n}\nverify { instances Claim = 2 values Amount = 0..3 }\n",
            Some("fsl5"),
            7,
            12,
            "ClaimStage",
        ),
    ] {
        let published = published_diagnostics(source, unstable);
        let errors = published
            .iter()
            .filter(|diagnostic| diagnostic["severity"] == 1)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{published:#?}");
        let error = errors[0];
        assert_eq!(error["code"], "FSL-SEMANTIC");
        assert_eq!(error["severity"], 1);
        assert_eq!(error["data"]["kind"], "semantics");
        assert_eq!(
            error["range"],
            json!({"start": {"line": line, "character": 4},
                   "end": {"line": line, "character": end}})
        );
        assert!(error["message"].as_str().is_some_and(|message| {
            message.contains(&format!(
                "expression of type Int is not assignable to Named(\"{type_name}\")"
            ))
        }));
    }
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

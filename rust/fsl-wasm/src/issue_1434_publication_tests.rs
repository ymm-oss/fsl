// SPDX-License-Identifier: Apache-2.0
//! Host admission controls, not evidence of browser solver/JSON dispatch.
//! test-browser.mjs owns actual Worker `run()` including wrongly typed old keys.
use std::future::Future;
use std::task::{Context, Poll, Waker};

use serde_json::{Value, json};

const BODY: &str = "spec S { state { flag: Bool = false } action stay() { flag = false } invariant Safe { not flag } }\n";
const VERSION: &str = "Z3 4.16.0.0";

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("host admission must not enter JavaScript solver"),
    }
}

fn expected(header: Option<u8>) -> Value {
    let mut value = json!({"fsl":"1.0","result":"ok","spec":"S","warnings":[],
        "versions":{"verifier":{"name":"fsl-wasm","version":env!("CARGO_PKG_VERSION")},
            "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
            "solver":{"name":"z3","backend":"z3-solver-wasm","version":VERSION}}});
    if let Some(edition) = header {
        value["fsl_edition"] = json!(edition);
    }
    if header == Some(5) {
        value["edition"] = json!("next");
    }
    value
}

fn request(source: &str, legacy: Option<Value>) -> Value {
    let mut value = json!({"cmd":"check","source":source,"source_file":"model.fsl"});
    if let Some(legacy) = legacy {
        value["unstable"] = legacy;
    }
    value
}

#[test]
fn publication_worker_check_ignores_all_legacy_field_types() {
    let mut failures = Vec::new();
    for (source, oracle) in [
        (BODY.to_owned(), expected(None)),
        (format!("fsl 4\n{BODY}"), expected(Some(4))),
        (format!("fsl 5\n{BODY}"), expected(Some(5))),
    ] {
        for legacy in [
            None,
            Some(json!(["fsl5"])),
            Some(json!(["later"])),
            Some(json!(42)),
            Some(json!({"fsl5":true})),
        ] {
            let packet = request(&source, legacy);
            let decoded = serde_json::from_value::<super::Request>(packet.clone());
            match decoded {
                Ok(decoded) => {
                    let actual = ready(super::check(&decoded, VERSION));
                    println!(
                        "{}",
                        json!({"request":packet,"actual":actual,"expected":oracle})
                    );
                    if actual != oracle {
                        failures.push(json!({"request":packet,"actual":actual,"expected":oracle}));
                    }
                }
                Err(error) => {
                    println!(
                        "{}",
                        json!({"request":packet,"decode_error":error.to_string()})
                    );
                    failures.push(json!({"request":packet,"decode_error":error.to_string()}));
                }
            }
        }
        let mut packet = request(&source, None);
        packet["unrelated_unknown"] = json!({"arbitrary":[42]});
        let decoded: super::Request = serde_json::from_value(packet).unwrap();
        let actual = ready(super::check(&decoded, VERSION));
        if actual != oracle {
            failures.push(json!({"unrelated_unknown":actual,"expected":oracle}));
        }
    }
    assert!(
        failures.is_empty(),
        "semantic publication RED: {failures:#?}"
    );
}

#[test]
fn publication_worker_known_field_types_still_refuse() {
    for (field, value) in [
        ("cmd", json!(42)),
        ("source", json!({})),
        ("source_file", json!(42)),
        ("files", json!([])),
        ("options", json!(42)),
    ] {
        let mut packet = request(BODY, None);
        packet[field] = value;
        let result = serde_json::from_value::<super::Request>(packet.clone());
        println!(
            "{}",
            json!({"request":packet,"decode":format!("{result:?}")})
        );
        assert!(result.is_err(), "malformed known {field} admitted");
    }
    for (field, value) in [("depth", json!("2")), ("deadlock", json!(42))] {
        let mut packet = request(BODY, None);
        packet["options"] = json!({field:value});
        assert!(serde_json::from_value::<super::Request>(packet).is_err());
    }
}

#[test]
fn publication_worker_refusal_preserves_scalar_not_utf16_location() {
    let source = "fsl 5\nspec S { state { flag: Bool = false }\n  @requirement(\"REQ-1\", \"日本語😀\") invariant I { true || false }\n}\n";
    let oracle = json!({"fsl":"1.0","result":"error","kind":"parse",
        "message":"expression operator '||' is unsupported in this edition; use 'or' manually, then review fslc migrate <file> --edition next for remaining edits at 3:52",
        "diagnostic_code":"unsupported_in_edition","loc":{"line":3,"column":52},
        "fsl_edition":5,"versions":expected(None)["versions"]});
    let mut outputs = Vec::new();
    for legacy in [Some(json!(["fsl5"])), None, Some(json!(["later"]))] {
        let packet = request(source, legacy);
        let decoded = serde_json::from_value::<super::Request>(packet.clone()).unwrap();
        let actual = ready(super::check(&decoded, VERSION));
        println!("{}", json!({"request":packet,"actual":actual}));
        outputs.push(actual);
    }
    for output in outputs {
        assert_eq!(output, oracle);
    }
}

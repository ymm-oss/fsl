// SPDX-License-Identifier: Apache-2.0
//! #1818 D1: the Worker reads a `fsl 5.N` header only when its request's
//! `options.unstable` names `fsl5-minor`, the way `FSLC_UNSTABLE` gates the
//! native CLI. Host admission only; the browser solver is not entered.
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

fn check(source: &str, options: &Value, top_level: Option<&Value>) -> Value {
    let mut packet =
        json!({"cmd":"check","source":source,"source_file":"model.fsl","options":options});
    if let Some(value) = top_level {
        packet["unstable"] = value.clone();
    }
    let request = serde_json::from_value::<super::Request>(packet).expect("request decodes");
    ready(super::check(&request, VERSION))
}

#[test]
fn the_worker_reads_the_development_minor_only_behind_its_request_gate() {
    let minor = format!("fsl 5.{}\n{BODY}", fsl_syntax::DEV_MINOR);
    let five = check(&format!("fsl 5\n{BODY}"), &json!({}), None);
    assert_eq!(five["result"], "ok", "{five:#}");
    assert_eq!(five["fsl_edition"], 5, "{five:#}");
    assert!(five.get("fsl_edition_minor").is_none(), "{five:#}");

    let gated = check(&minor, &json!({"unstable":["fsl5-minor"]}), None);
    let mut expected = five.clone();
    expected["fsl_edition_minor"] = json!(fsl_syntax::DEV_MINOR);
    assert_eq!(gated, expected, "the minor document reads as the fsl 5 one");
    // The gate does not change a published document.
    assert_eq!(
        check(
            &format!("fsl 5\n{BODY}"),
            &json!({"unstable":["fsl5-minor"]}),
            None
        ),
        five
    );

    // Without the gate (or with a malformed or misplaced one) the header is
    // an unsupported edition, never a request error.
    for (options, top_level) in [
        (json!({}), None),
        (json!({"unstable":[]}), None),
        (json!({"unstable":["fsl5"]}), None),
        (json!({"unstable":"fsl5-minor"}), None),
        (json!({"unstable":42}), None),
        (json!({"unstable":{"fsl5-minor":true}}), None),
        (json!({}), Some(json!(["fsl5-minor"]))),
    ] {
        let output = check(&minor, &options, top_level.as_ref());
        assert_eq!(
            output["result"], "error",
            "{options} {top_level:?}: {output:#}"
        );
        assert_eq!(
            output["diagnostic_code"], "unsupported_edition",
            "{options} {top_level:?}: {output:#}"
        );
    }
}

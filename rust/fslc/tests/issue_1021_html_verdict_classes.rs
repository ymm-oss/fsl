// SPDX-License-Identifier: Apache-2.0

//! Issue #1021: `fslc html` draws a `verify` envelope's top-level `result` as
//! a badge, classified in `fsl-tools`, which cannot see the verdict
//! vocabulary's owner (`fslc_rust::outcome`). This test lives on the side
//! that sees both and checks every verdict `fsl-tools` lists against the
//! owner: a pass is drawn `ok`, a non-pass `bad` or `warn`, and each listed
//! value is one `verify` can publish, not an unregistered one.

use fslc_rust::outcome::{OutcomeClass, exit_status, outcome_class};
use serde_json::json;

#[test]
fn html_verdict_classes_agree_with_outcome_class() {
    for result in fsl_tools::VERIFY_VERDICTS {
        let envelope = json!({ "result": result });
        let class = fsl_tools::verdict_class(result)
            .unwrap_or_else(|| panic!("{result}: listed without a class"));
        let outcome = outcome_class(&envelope);
        match class {
            "ok" => assert_eq!(outcome, OutcomeClass::Success, "{result}"),
            "bad" | "warn" => assert_eq!(outcome, OutcomeClass::Failure, "{result}"),
            other => panic!("{result}: verdict drawn as {other}"),
        }
        // 3 is exit_status's answer for a value outside the verify vocabulary.
        assert_ne!(
            exit_status(&envelope, 2),
            3,
            "{result}: not a verify verdict"
        );
    }
}

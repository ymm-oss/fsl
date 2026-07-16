// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, governance_contract};

struct MemoryResolver(BTreeMap<String, String>);

impl FileResolver for MemoryResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        self.0.get(path).cloned().ok_or_else(|| CoreError {
            message: format!("missing {path}"),
            line: 1,
            column: 1,
            origin: None,
        })
    }
}

const BUSINESS: &str = r#"business Flow {
  actor User
  entity Item
  process Item {
    stages Open, Done
    initial Open
    transition finish Open -> Done by User
  }
  policy P-1 "Finish every item" satisfies CTRL-1, CTRL-1
    every Item in Open must eventually be Done
  policy P-LOCAL "Business-local metadata" satisfies CTRL-LOCAL
    every Item in Open must eventually be Done
  goal G-1 "Completion is reachable"
    some Item can reach Done
}
verify { instances Item = 1 }
"#;

fn resolver() -> MemoryResolver {
    MemoryResolver(BTreeMap::from([
        ("flow.fsl".to_owned(), BUSINESS.to_owned()),
        (
            "before.fsl".to_owned(),
            "spec Before { state { done: Bool } init { done = false } }".to_owned(),
        ),
        (
            "after.fsl".to_owned(),
            "spec After { state { done: Bool } init { done = false } }".to_owned(),
        ),
        (
            "mapping.fsl".to_owned(),
            "refinement Mapping { impl After abs Before maps auto }".to_owned(),
        ),
        ("malformed.fsl".to_owned(), "business Broken {".to_owned()),
    ]))
}

#[test]
fn delegate_resolves_implicit_and_merged_explicit_satisfaction() {
    let source = r#"governance Controls {
  control CTRL-1 "Items finish"
  delegates Flow from "flow.fsl" {
    CTRL-1 is satisfied_by goal G-1
    require CTRL-1
    CTRL-1 is satisfied_by policy P-1
  }
}
"#;

    let contract = governance_contract(source, &resolver())
        .expect("valid governance contract")
        .expect("governance document");

    assert_eq!(
        contract.delegates[0].satisfied["CTRL-1"],
        vec![
            ("policy".to_owned(), "P-1".to_owned()),
            ("goal".to_owned(), "G-1".to_owned()),
        ]
    );
    assert_eq!(
        contract.delegates[0].satisfied.keys().collect::<Vec<_>>(),
        vec!["CTRL-1"]
    );
}

#[test]
fn delegate_rejects_unknown_unsatisfied_and_missing_dependencies() {
    for (source, expected) in [
        (
            r#"governance Controls {
  delegates Flow from "flow.fsl" { require CTRL-UNKNOWN }
}"#,
            "unknown governance control 'CTRL-UNKNOWN'",
        ),
        (
            r#"governance Controls {
  control CTRL-2 "Unclaimed"
  delegates Flow from "flow.fsl" { require CTRL-2 }
}"#,
            "governance control 'CTRL-2' is not satisfied",
        ),
        (
            r#"governance Controls {
  control CTRL-1 "Items finish"
  delegates Flow from "missing.fsl" { require CTRL-1 }
}"#,
            "governance dependency not found: 'missing.fsl'",
        ),
        (
            r#"governance Controls {
  control CTRL-1 "Items finish"
  delegates Broken from "malformed.fsl" { require CTRL-1 }
}"#,
            "invalid governance dependency 'malformed.fsl'",
        ),
    ] {
        let error = governance_contract(source, &resolver()).expect_err("contract must fail");
        assert!(
            error.message.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn governance_rejects_ambiguous_or_unknown_control_declarations() {
    for (source, expected) in [
        (
            r#"governance Controls {
  control CTRL-1 "First"
  control CTRL-1 "Second"
}"#,
            "duplicate governance control 'CTRL-1'",
        ),
        (
            r"governance Controls {
  authority Team owns CTRL-MISSING
}",
            "unknown governance control 'CTRL-MISSING'",
        ),
    ] {
        let error = governance_contract(source, &resolver()).expect_err("contract must fail");
        assert!(
            error.message.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn preservation_rejects_ambiguous_empty_unknown_and_misnamed_contracts() {
    for (source, expected) in [
        (
            r#"governance Controls {
  control CTRL-1 "Items finish"
  preservation P {
    before Before from "before.fsl"
    before Before from "before.fsl"
    after After from "after.fsl"
    preserve CTRL-1
    checked_by refinement "mapping.fsl"
  }
}"#,
            "duplicate governance preservation before",
        ),
        (
            r#"governance Controls {
  preservation P {
    before Before from "before.fsl"
    after After from "after.fsl"
    checked_by refinement "mapping.fsl"
  }
}"#,
            "must preserve at least one control",
        ),
        (
            r#"governance Controls {
  preservation P {
    before Before from "before.fsl"
    after After from "after.fsl"
    preserve CTRL-MISSING
    checked_by refinement "mapping.fsl"
  }
}"#,
            "unknown governance control 'CTRL-MISSING'",
        ),
        (
            r#"governance Controls {
  control CTRL-1 "Items finish"
  preservation P {
    before WrongName from "before.fsl"
    after After from "after.fsl"
    preserve CTRL-1
    checked_by refinement "mapping.fsl"
  }
}"#,
            "expects 'WrongName', found 'Before'",
        ),
    ] {
        let error = governance_contract(source, &resolver()).expect_err("contract must fail");
        assert!(
            error.message.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }
}

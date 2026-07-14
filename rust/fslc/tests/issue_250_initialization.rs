// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-250-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }

    fn replace(&self, source: &str) {
        std::fs::write(&self.0, source).expect("replace fixture");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn without_spans(mut value: Value) -> Value {
    match &mut value {
        Value::Object(object) => {
            object.remove("span");
            for child in object.values_mut() {
                *child = without_spans(std::mem::take(child));
            }
        }
        Value::Array(items) => {
            for item in items {
                *item = without_spans(std::mem::take(item));
            }
        }
        _ => {}
    }
    value
}

const INLINE: &str = r"spec InlineInit {
  const ZERO = 0
  enum Status { Pending, Done }
  type Count = 0..2
  type ItemId = 0..1
  type JobId = 0..1
  state {
    status: Status = Pending,
    count: Count = ZERO,
    current: Option<ItemId> = none,
    queue: Seq<JobId, 3> = Seq {},
  }
  action finish() { status = Done }
  invariant CountStartsAtZero { count >= 0 }
}
";

const EXPLICIT: &str = r"spec InlineInit {
  const ZERO = 0
  enum Status { Pending, Done }
  type Count = 0..2
  type ItemId = 0..1
  type JobId = 0..1
  state {
    status: Status,
    count: Count,
    current: Option<ItemId>,
    queue: Seq<JobId, 3>,
  }
  init {
    status = Pending
    count = ZERO
    current = none
    queue = Seq {}
  }
  action finish() { status = Done }
  invariant CountStartsAtZero { count >= 0 }
}
";

#[test]
fn inline_and_explicit_forms_share_checked_init_and_verdicts() {
    let inline = Fixture::new("inline", INLINE);
    let explicit = Fixture::new("explicit", EXPLICIT);

    let (inline_kernel, inline_status) = run(&["kernel", inline.text()]);
    let (explicit_kernel, explicit_status) = run(&["kernel", explicit.text()]);
    assert_eq!(inline_status, 0, "{inline_kernel}");
    assert_eq!(explicit_status, 0, "{explicit_kernel}");
    assert!(inline_kernel["init"]["statements"].is_array());
    assert_eq!(
        without_spans(inline_kernel["init"]["statements"].clone()),
        without_spans(explicit_kernel["init"]["statements"].clone())
    );

    for engine in ["bmc", "induction", "explicit"] {
        let (inline_result, inline_status) = run(&[
            "verify",
            inline.text(),
            "--depth",
            "2",
            "--engine",
            engine,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        let (explicit_result, explicit_status) = run(&[
            "verify",
            explicit.text(),
            "--depth",
            "2",
            "--engine",
            engine,
            "--deadlock",
            "ignore",
            "--no-cache",
        ]);
        assert_eq!(inline_status, explicit_status, "{engine}: {inline_result}");
        assert_eq!(inline_result["result"], explicit_result["result"]);
        assert_eq!(
            inline_result["completeness"],
            explicit_result["completeness"]
        );
    }
}

#[test]
fn inline_assignments_precede_the_logical_init_regardless_of_source_order() {
    let fixture = Fixture::new(
        "source-order",
        r"spec SourceOrder {
  type N = 0..2
  init { second = 1 }
  state { first: N = 0, second: N }
  action stay() { second = second }
  invariant InRange { first >= 0 }
}
",
    );
    let (kernel, status) = run(&["kernel", fixture.text()]);
    assert_eq!(status, 0, "{kernel}");
    let statements = kernel["init"]["statements"]
        .as_array()
        .expect("init statements");
    assert_eq!(statements[0]["target"]["name"], "first");
    assert_eq!(statements[1]["target"]["name"], "second");
}

#[test]
fn inline_initializer_must_not_read_any_state_root() {
    for source in [
        "spec Bad { type N = 0..2 state { a: N = 0, b: N = a } action stay() { b = b } }",
        "spec Bad { type N = 0..2 state { a: N = a } action stay() { a = a } }",
    ] {
        let fixture = Fixture::new("state-read", source);
        let (output, status) = run(&["check", fixture.text()]);
        assert_eq!(status, 2, "{output}");
        assert_eq!(output["kind"], "semantics");
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|message| message.contains("must not read state"))
        );
    }
}

#[test]
fn inline_values_use_the_existing_name_and_type_checker() {
    for (inline, explicit) in [
        (
            "spec Bad { state { flag: Bool = 0 } action stay() { } }",
            "spec Bad { state { flag: Bool } init { flag = 0 } action stay() { } }",
        ),
        (
            "spec Bad { enum Status { Pending } state { status: Status = Missing } action stay() { } }",
            "spec Bad { enum Status { Pending } state { status: Status } init { status = Missing } action stay() { } }",
        ),
        (
            "spec Bad { state { count: Int = 1 + Missing } action stay() { } }",
            "spec Bad { state { count: Int } init { count = 1 + Missing } action stay() { } }",
        ),
    ] {
        for source in [inline, explicit] {
            let fixture = Fixture::new("invalid-value", source);
            let (output, status) = run(&["check", fixture.text()]);
            assert_eq!(status, 2, "{output}");
            assert!(
                matches!(output["kind"].as_str(), Some("type" | "semantics")),
                "{output}"
            );
        }
    }
}

#[test]
fn shared_init_checker_uses_runtime_state_name_precedence() {
    let fixture = Fixture::new(
        "name-precedence",
        r"spec NamePrecedence {
  const source = 0
  state { source: Bool, target: Bool }
  init { source = false target = source }
  action stay() { target = target }
  invariant Typed { target == true or target == false }
}
",
    );
    let (output, status) = run(&["check", fixture.text()]);
    assert_eq!(status, 0, "{output}");
}

#[test]
fn inline_and_explicit_assignment_to_the_same_root_reports_both_spans() {
    let fixture = Fixture::new(
        "overlap",
        r"spec Bad {
  type N = 0..2
  state { count: N = 0 }
  init { count = 1 }
  action stay() { count = count }
}
",
    );
    let (output, status) = run(&["check", fixture.text()]);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["kind"], "semantics");
    let message = output["message"].as_str().expect("semantic message");
    assert!(message.contains("at 3:"), "{message}");
    assert!(
        message.contains("conflicting assignment at 4:"),
        "{message}"
    );
}

#[test]
fn relational_and_bulk_initialization_remain_init_only() {
    for declaration in [
        "values: Map<N, N> = forall n: N { values[n] = 0 }",
        "value: N = if true { value = 0 } else { value = 1 }",
    ] {
        let source =
            format!("spec Bad {{ type N = 0..1 state {{ {declaration} }} action stay() {{ }} }}");
        let fixture = Fixture::new("statement-form", &source);
        let (output, status) = run(&["check", fixture.text()]);
        assert_eq!(status, 2, "{output}");
        assert_eq!(output["kind"], "parse");
    }
}

#[test]
fn quantified_expression_initializers_are_rejected_semantically() {
    let fixture = Fixture::new(
        "quantified-expression",
        r"spec Quantified {
  type N = 0..1
  state { n: N, flag: Bool = forall x: N: x == x }
  init { n = 0 }
  action stay() { n = n }
}
",
    );
    let (output, status) = run(&["check", fixture.text()]);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["kind"], "semantics");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("quantified expression")),
        "{output}"
    );
}

#[test]
fn domain_implicit_values_warn_with_selected_values_and_insertions() {
    let source = r"domain Defaults {
  implementation_profile functional_ddd
  enum Status { Pending, Done }
  type Count = 2..3
  aggregate Order {
    id OrderId
    state {
      status: Status;
      active: Bool;
      count: Count;
      owner: OwnerId; // keep this comment
    }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
  }
}
";
    let fixture = Fixture::new("domain-defaults", source);
    let (output, status) = run(&["check", fixture.text()]);
    assert_eq!(status, 0, "{output}");
    let warnings = output["warnings"].as_array().expect("warnings array");
    let implicit = warnings
        .iter()
        .filter(|warning| warning["code"] == "implicit_initial_value")
        .collect::<Vec<_>>();
    assert_eq!(implicit.len(), 4, "{output}");
    let selected = implicit
        .iter()
        .map(|warning| {
            (
                warning["field"].as_str().expect("field"),
                warning["selected_value"].as_str().expect("selected value"),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(selected["Order.status"], "Pending");
    assert_eq!(selected["Order.active"], "false");
    assert_eq!(selected["Order.count"], "2");
    assert_eq!(selected["Order.owner"], "0");
    for warning in implicit {
        assert_eq!(warning["edition_severity"]["current"], "warning");
        assert_eq!(warning["edition_severity"]["next"], "error");
        assert_eq!(warning["suggestion"]["kind"], "insert");
        assert_eq!(warning["suggestion"]["machine_applicable"], true);
        assert_eq!(
            warning["suggestion"]["span"]["start"],
            warning["suggestion"]["span"]["end"]
        );
    }
}

#[test]
fn requirements_number_default_warning_edit_preserves_comment_and_verdict() {
    let source = r"requirements Amounts {
  number Amount
  process Claim with amount: Amount // keep
  {
    stages Draft, Done
    initial Draft
    transition finish Draft -> Done by System
  }
}
verify {
  instances Claim = 1
  values Amount = 2..4
}
";
    let fixture = Fixture::new("requirements-default", source);
    let (before, status) = run(&["check", fixture.text()]);
    assert_eq!(status, 0, "{before}");
    let warning = before["warnings"]
        .as_array()
        .and_then(|warnings| {
            warnings
                .iter()
                .find(|warning| warning["code"] == "implicit_initial_value")
        })
        .unwrap_or_else(|| panic!("missing implicit warning: {before}"));
    assert_eq!(warning["field"], "Claim.amount");
    assert_eq!(warning["selected_value"], "2");

    let mut migrated = source.to_owned();
    let start = usize::try_from(
        warning["suggestion"]["span"]["start"]
            .as_u64()
            .expect("byte offset"),
    )
    .expect("offset fits usize");
    migrated.insert_str(
        start,
        warning["suggestion"]["replacement"]
            .as_str()
            .expect("replacement"),
    );
    assert!(migrated.contains("amount: Amount = 2 // keep"));
    fixture.replace(&migrated);

    let (after, status) = run(&["check", fixture.text()]);
    assert_eq!(status, 0, "{after}");
    assert!(!after["warnings"].as_array().is_some_and(|warnings| {
        warnings
            .iter()
            .any(|warning| warning["code"] == "implicit_initial_value")
    }));
}

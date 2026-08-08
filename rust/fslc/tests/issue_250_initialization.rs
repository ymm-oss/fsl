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

fn characterization_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/domain_characterization/{name}"))
}

fn implicit_warnings(output: &Value) -> std::collections::BTreeMap<String, Value> {
    output["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .filter(|warning| warning["code"] == "implicit_initial_value")
        .map(|warning| {
            (
                warning["field"].as_str().expect("field").to_owned(),
                warning.clone(),
            )
        })
        .collect()
}

/// `fslc domain expand PATH` prints the generated kernel source as raw text
/// (not JSON) whenever the command succeeds and no `--output` is given, so
/// route through `--output` and read the written file back instead of
/// reusing `run`'s JSON-only decoder. `output` is a [`Fixture`] purely for
/// its `Drop`-based cleanup (issue #731 review, n3): it reserves the temp
/// path up front so the file is removed even if an `assert!` below panics,
/// matching every other temp file in this suite instead of a bare
/// `remove_file` that a panic would skip.
fn domain_expand_source(path_text: &str) -> String {
    let output = Fixture::new("expand-output", "");
    let (result, status) = run(&["domain", "expand", path_text, "--output", output.text()]);
    assert_eq!(status, 0, "{result}");
    std::fs::read_to_string(&output.0).expect("read expanded kernel source")
}

/// Issue #731: `implicit_initial_value` must fire for container-typed domain
/// state fields (`Option<T>`, `Set<T>`, a top-level `Map<K, V>`), not just
/// the four scalar shapes, because `domain_kernel_source`'s renderer already
/// gives every one of these an implicit default. The `selected_value` this
/// warning reports must match `fslc domain expand`'s rendered default at the
/// string level -- both must come from the same `fsl_core::domain_type_default`
/// owner, not a second hand-rolled copy of the dispatch.
#[test]
fn domain_container_fields_warn_and_match_the_renderer_default() {
    let path = characterization_fixture("container_defaults_surface.fsl");
    let path_text = path.to_str().expect("UTF-8 path");
    let (output, status) = run(&["check", path_text]);
    assert_eq!(status, 0, "{output}");
    let implicit = implicit_warnings(&output);
    assert_eq!(
        implicit.keys().collect::<Vec<_>>(),
        vec!["Basket.picked", "Basket.seen"],
        "{output}"
    );

    let picked = &implicit["Basket.picked"];
    assert_eq!(picked["selected_value"], "none");
    assert_eq!(picked["suggestion"]["machine_applicable"], true);
    assert_eq!(picked["edition_severity"]["next"], "error");

    let seen = &implicit["Basket.seen"];
    assert_eq!(seen["selected_value"], "Set {}");
    // No machine-applicable insertion for a brace-literal default (issue
    // #770 tracks the `fslc fmt` round-trip defect this withholds against);
    // `--edition next` cannot yet demand an initializer it cannot safely
    // insert.
    assert!(seen.get("suggestion").is_none(), "{seen}");
    assert!(seen.get("canonical_replacement").is_none(), "{seen}");
    assert_eq!(seen["edition_severity"]["next"], "warning");

    let expanded = domain_expand_source(path_text);
    let picked_value = picked["selected_value"].as_str().expect("picked value");
    let seen_value = seen["selected_value"].as_str().expect("seen value");
    assert!(
        expanded.contains(&format!("basket_picked = {picked_value}")),
        "{expanded}"
    );
    assert!(
        expanded.contains(&format!("basket_seen = {seen_value}")),
        "{expanded}"
    );
}

/// Companion to the Option/Set case above: a top-level `Map<K, V>` field has
/// no whole-field default at all (only the per-key `forall` init the
/// renderer builds), and a `value_object`-typed field's struct-literal
/// default hits the same #770 formatter round-trip gap `Set {}` does. Both
/// must still warn under `check` and both must stay excluded from
/// `--edition next` enforcement, since neither has a safe insertion to
/// demand.
#[test]
fn domain_map_and_value_object_fields_warn_without_a_next_edition_deadline() {
    let path = characterization_fixture("lvalues_surface.fsl");
    let path_text = path.to_str().expect("UTF-8 path");
    let (output, status) = run(&["check", path_text]);
    assert_eq!(status, 0, "{output}");
    let implicit = implicit_warnings(&output);
    assert_eq!(
        implicit.keys().collect::<Vec<_>>(),
        vec!["Inventory.counter", "Inventory.counts"],
        "{output}"
    );
    // `total: Quantity = 0;` has an explicit default and must not warn.
    assert!(!implicit.contains_key("Inventory.total"), "{output}");

    let counts = &implicit["Inventory.counts"];
    assert_eq!(counts["selected_value"], "0");
    assert!(counts.get("suggestion").is_none(), "{counts}");
    assert_eq!(counts["edition_severity"]["next"], "warning");

    let counter = &implicit["Inventory.counter"];
    assert_eq!(counter["selected_value"], "Counter { value: 0 }");
    assert!(counter.get("suggestion").is_none(), "{counter}");
    assert_eq!(counter["edition_severity"]["next"], "warning");

    let expanded = domain_expand_source(path_text);
    assert!(expanded.contains("inventory_counts[k] = 0"), "{expanded}");
    assert!(
        expanded.contains("inventory_counter = Counter { value: 0 }"),
        "{expanded}"
    );

    let (next, next_status) = run(&["check", path_text, "--edition", "next"]);
    assert_eq!(next_status, 0, "{next}");
    assert_eq!(next["result"], "ok", "{next}");
}

/// Issue #731 review, M1: an enum nested one level below a field's own type
/// -- inside a `value_object`'s struct literal, or as a top-level `Map`'s
/// per-key value -- must still report the bare domain-source member name,
/// not `domain_kernel_source`'s kernel-mangled `Enum_Member` identifier. An
/// earlier version of this fix special-cased only a field's own top-level
/// enum type in `frontend_output.rs`, which still mangled an enum nested one
/// layer down; the fix now lives in `fsl_core::domain_type_default` itself
/// (the single owner), so nesting depth cannot reintroduce the bug.
#[test]
fn domain_nested_enum_defaults_stay_bare_inside_value_object_and_map() {
    let source = r"domain NestedEnumDefaults {
  enum OrderStatus { Draft, Placed }
  type ItemId = 0..1
  value_object AuditStamp {
    status: OrderStatus;
    attempts: Int;
  }
  aggregate Order {
    id ItemId
    state {
      audit: AuditStamp;
      history: Map<ItemId, OrderStatus>;
    }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
  }
}
";
    let fixture = Fixture::new("nested-enum-defaults", source);
    let path_text = fixture.text();
    let (output, status) = run(&["check", path_text]);
    assert_eq!(status, 0, "{output}");
    let implicit = implicit_warnings(&output);

    let audit = &implicit["Order.audit"];
    assert_eq!(
        audit["selected_value"], "AuditStamp { status: Draft, attempts: 0 }",
        "{audit}"
    );
    assert!(
        !audit["selected_value"]
            .as_str()
            .expect("audit value")
            .contains("OrderStatus_"),
        "{audit}"
    );

    let history = &implicit["Order.history"];
    assert_eq!(history["selected_value"], "Draft", "{history}");
    assert!(
        !history["selected_value"]
            .as_str()
            .expect("history value")
            .contains("OrderStatus_"),
        "{history}"
    );

    // `AuditStamp { ... }` is itself a brace-literal value_object default
    // (issue #770), so it must not be offered as a machine-applicable
    // insertion regardless of the enum fix above.
    assert!(audit.get("suggestion").is_none(), "{audit}");
}

/// Issue #731 review, M3: `Int` hit the exact defect class #731 fixed for
/// containers -- the renderer (`Context::default_for_type`'s `"Int" => Ok("0")`
/// arm) always selects `0` for an omitted `Int` field, but the warning
/// carved `Int` out unconditionally. `0` contains no brace, so it is safe to
/// offer as a machine-applicable insertion like every other scalar shape.
#[test]
fn domain_bare_int_fields_warn_like_every_other_scalar_shape() {
    let source = r"domain BareIntDefault {
  type ItemId = 0..1
  aggregate Counter {
    id ItemId
    state {
      raw: Int;
      flag: Bool;
    }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
  }
}
";
    let fixture = Fixture::new("bare-int-default", source);
    let path_text = fixture.text();
    let (output, status) = run(&["check", path_text]);
    assert_eq!(status, 0, "{output}");
    let implicit = implicit_warnings(&output);
    assert_eq!(
        implicit.keys().collect::<Vec<_>>(),
        vec!["Counter.flag", "Counter.raw"],
        "{output}"
    );
    let raw = &implicit["Counter.raw"];
    assert_eq!(raw["selected_value"], "0");
    assert_eq!(raw["suggestion"]["machine_applicable"], true);
    assert_eq!(raw["edition_severity"]["next"], "error");

    let expanded = domain_expand_source(path_text);
    assert!(expanded.contains("counter_raw = 0"), "{expanded}");
}

/// Issue #731 review, m2: a "defect witness" for #770, the pre-existing
/// `fslc fmt` round-trip defect this PR discovered and withholds a
/// machine-applicable insertion against for `Set<T>`/`value_object`
/// defaults. This test does not exercise this PR's own code at all -- it
/// pins #770's *symptom* directly. If #770 is ever fixed, this assertion
/// starts failing, which is the intended signal to revisit
/// `insertable_shape` in `frontend_output.rs` and re-enable the insertion
/// these two shapes currently withhold, rather than #770's fix landing
/// silently while the withholding logic here quietly outlives its reason.
///
/// #770 fixed is necessary but not sufficient for that re-enable: issue
/// #785 (found independently during this PR's second review round, and not
/// reachable through this PR's own withheld insertion) is a second,
/// pre-existing defect on the same `Context::normalize` enum-mangling guard
/// this PR's `DefaultForm` fix touches for M1 -- an *explicit* brace-literal
/// default containing an enum member (e.g. a hand-written
/// `audit: AuditStamp = AuditStamp { status: Red, attempts: 0 };`) never
/// gets that member mangled for the generated kernel, because the guard
/// only fires for a bare alnum/underscore identifier, not a member embedded
/// in a larger expression; the resulting kernel text fails
/// `fslc check`. Re-enabling the `value_object` insertion once #770 is
/// fixed but #785 is not would let `migrate --write` insert exactly that
/// shape -- a `value_object` struct literal whose bare enum members
/// `fsl_core::domain_type_default` rendered correctly for the *warning*
/// stay unmangled the next time `domain_kernel_source` re-renders that now
/// explicit default, producing a file `check` accepts today but whose
/// generated kernel `check` rejects after the edit. Confirm #785 is fixed
/// too before flipping `insertable_shape`'s `value_object` arm.
#[test]
fn domain_brace_literal_defaults_still_fail_the_770_fmt_round_trip() {
    for source in [
        r"domain SetLiteralWitness {
  type ItemId = 0..1
  aggregate Basket {
    id ItemId
    state { seen: Set<ItemId> = Set {}; }
    command Pick {}
    event Picked {}
    decide Pick { emits Picked }
    evolve Picked {}
  }
}
",
        r"domain ValueObjectLiteralWitness {
  type Quantity = 0..2
  value_object Counter { value: Quantity = 0; }
  aggregate Inventory {
    id Quantity
    state { counter: Counter = Counter { value: 0 }; }
    command Adjust {}
    event Adjusted {}
    decide Adjust { emits Adjusted }
    evolve Adjusted {}
  }
}
",
    ] {
        let fixture = Fixture::new("770-witness", source);
        let (output, status) = run(&["fmt", fixture.text(), "--check"]);
        assert_eq!(
            status, 2,
            "issue #770 appears fixed -- {output}. Before re-enabling the \
             machine-applicable insertion `insertable_shape` withholds for this \
             shape in rust/fslc/src/frontend_output.rs, also confirm issue #785 \
             is fixed (a second, pre-existing defect on the same \
             `Context::normalize` enum-mangling guard in rust/fsl-core/src/domain.rs: \
             an explicit brace-literal default containing an enum member never \
             gets that member mangled for the generated kernel, so re-enabling \
             only on #770's fix would let `migrate --write` insert a \
             `value_object` default that `check` accepts today but whose \
             generated kernel `check` rejects after the edit) -- then update \
             docs/LANGUAGE.md, docs/LANGUAGE.ja.md, and skills/fsl/reference.md \
             accordingly."
        );
        assert_eq!(output["kind"], "parse", "{output}");
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

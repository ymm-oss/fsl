// SPDX-License-Identifier: Apache-2.0

//! Entity-scoping controls for `fslc typestate` struct-field state machines
//! (issue #520). Struct-field locations were keyed by field *name* only, so
//! two entities that both declare a field called `status` each received the
//! other's transitions — `--ts` then emitted a method against the wrong
//! host type.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn run_typestate_json(source: &str) -> Value {
    let dir = tempfile_dir();
    let path = dir.join("spec.fsl");
    std::fs::write(&path, source).expect("write fixture spec");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("typestate")
        .arg(&path)
        .output()
        .expect("run native typestate CLI");
    assert!(
        output.status.success(),
        "typestate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn tempfile_dir() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-typestate-entity-scoping-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch dir");
    directory
}

fn entity<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["entities"]
        .as_array()
        .expect("entities array")
        .iter()
        .find(|entity| entity["entity"] == key)
        .unwrap_or_else(|| panic!("no entity {key} in {report}"))
}

fn action_names(entity: &Value) -> Vec<&str> {
    entity["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .map(|action| action["action"].as_str().expect("action name"))
        .collect()
}

/// Negative control for #520: `Order` and `Ticket` both declare a field
/// named `status` backed by the *same* enum type, so the old field-name-only
/// scoping could not tell them apart. `finish_order` must appear only under
/// `Order`, and `close_ticket` only under `Ticket`.
#[test]
fn same_field_name_and_enum_keeps_each_entity_to_its_own_transitions() {
    let source = r"
spec SharedField {
  enum St { Open, Closed }
  struct Order { status: St }
  struct Ticket { status: St }
  state { order: Order, ticket: Ticket }
  init {
    order = Order { status: Open }
    ticket = Ticket { status: Open }
  }
  action finish_order() {
    requires order.status == Open
    order.status = Closed
  }
  action close_ticket() {
    requires ticket.status == Open
    ticket.status = Closed
  }
}
";
    let report = run_typestate_json(source);

    let order = entity(&report, "Order.status");
    assert_eq!(action_names(order), vec!["finish_order"]);
    assert!(
        order["typescript"]
            .as_str()
            .expect("typescript")
            .contains("export function finish_order"),
    );
    assert!(
        !order["typescript"]
            .as_str()
            .expect("typescript")
            .contains("close_ticket"),
        "Order's generated TypeScript must not carry Ticket's close_ticket transition"
    );

    let ticket = entity(&report, "Ticket.status");
    assert_eq!(action_names(ticket), vec!["close_ticket"]);
    assert!(
        ticket["typescript"]
            .as_str()
            .expect("typescript")
            .contains("export function close_ticket"),
    );
    assert!(
        !ticket["typescript"]
            .as_str()
            .expect("typescript")
            .contains("finish_order"),
        "Ticket's generated TypeScript must not carry Order's finish_order transition"
    );
}

/// Positive control for #520: multiple actions over the *same* entity's
/// `status` field must still aggregate into one machine — scoping by owner
/// type must not fragment a single entity's own transitions.
#[test]
fn multiple_actions_over_one_entity_status_still_aggregate() {
    let source = r"
spec OneEntityTwoActions {
  enum St { Draft, Placed, Done }
  struct Order { status: St }
  state { order: Order }
  init { order = Order { status: Draft } }
  action place() {
    requires order.status == Draft
    order.status = Placed
  }
  action finish() {
    requires order.status == Placed
    order.status = Done
  }
}
";
    let report = run_typestate_json(source);
    assert_eq!(report["entities"].as_array().expect("entities").len(), 1);
    let order = entity(&report, "Order.status");
    assert_eq!(action_names(order), vec!["place", "finish"]);
    assert_eq!(order["applicability"], "full");
}

/// Negative control for #520's sibling defect in the whole-struct-literal
/// reassignment path (`ticket = Ticket { status: Closed }`, as opposed to
/// `ticket.status = Closed`). That path matched a struct literal's field
/// name against `entity.field` without checking which struct the literal
/// itself declares, so `Order.status` (unrelated to `close_ticket`) still
/// absorbed the transition. `Order.status` has zero real transitions here,
/// so it must not appear in the report at all.
#[test]
fn same_field_name_keeps_struct_literal_reassignment_scoped_too() {
    let source = r"
spec StructLitContamination {
  enum St { Open, Closed }
  struct Order { status: St }
  struct Ticket { status: St }
  state { order: Order, ticket: Ticket }
  init {
    order = Order { status: Open }
    ticket = Ticket { status: Open }
  }
  action close_ticket() {
    requires ticket.status == Open
    ticket = Ticket { status: Closed }
  }
}
";
    let report = run_typestate_json(source);
    let entities = report["entities"].as_array().expect("entities");
    assert_eq!(
        entities.iter().map(|e| &e["entity"]).collect::<Vec<_>>(),
        vec!["Ticket.status"],
        "Order.status has no real transitions and must not appear: {report}"
    );
    let ticket = entity(&report, "Ticket.status");
    assert_eq!(action_names(ticket), vec!["close_ticket"]);
    assert_eq!(ticket["actions"][0]["verdict"], "derivable");
}

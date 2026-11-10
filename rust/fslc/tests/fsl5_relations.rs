// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S8, through the native CLI and the library: `R belongs to E
//! as r via n`, `resource`, `measure`, `goal` and `policy` in a document
//! headed `fsl 5`.
//!
//! The acceptance (K, R and M for the multi-entity example against
//! `original_renamed.fsl`, and K for the hand-written `relations` example)
//! lives in `fsl5_lowering_contract.rs`. This file holds what the Public
//! Kernel does not show:
//!
//! - A measure is a projection: the count the design draft's refinement
//!   mapping writes for the counter it replaces, with the concept's `where`
//!   condition carried and type-checked.
//! - A scenario step names the instance a relation records.
//! - A guard that reads a resource is decided by the enumeration; one that
//!   reads a relation role is left to the solver, which reads the role's
//!   state.
//! - The declarations' errors.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir(example: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(example)
}

fn fixture(example: &str, file: &str) -> String {
    std::fs::read_to_string(fixture_dir(example).join(file)).expect("read fixture")
}

/// A scratch directory for one test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fsl5_relations")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).expect("write scratch source");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `fslc <command> <source>` on `source` written to a scratch file.
fn fslc(command: &str, label: &str, source: &str) -> (Value, i32) {
    fslc_with(&[command, "doc.fsl"], label, source)
}

/// `fslc <args>` with `source` written to `doc.fsl` in a scratch directory.
fn fslc_with(args: &[&str], label: &str, source: &str) -> (Value, i32) {
    let command = args[0];
    let scratch = Scratch::new(label);
    scratch.write("doc.fsl", source);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(&scratch.0)
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{command} {label}: invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

/// `source` with `old` (which occurs exactly once) replaced by `new`.
fn edit(source: &str, old: &str, new: &str) -> String {
    assert_eq!(source.matches(old).count(), 1, "{old:?}");
    source.replacen(old, new, 1)
}

/// The kernel model of a next-edition document.
fn model(source: &str) -> fsl_core::KernelModel {
    let options = fsl_syntax::ParseOptions::stable();
    let kernel = fsl_core::parse_kernel_source_with_file_with_options(
        source,
        &fsl_core::FsResolver::new(fixture_dir("multi_entity")),
        "doc.fsl",
        &options,
    )
    .expect("lower");
    fsl_core::build_model(kernel).expect("model")
}

fn whitespace_collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The expression the design draft's mapping `map <counter> = …` gives a
/// counter of the original, in `refines_lowered_to_original.fsl`.
fn mapped_count(counter: &str) -> String {
    let mapping = fixture("multi_entity", "refines_lowered_to_original.fsl");
    let start = mapping
        .find(&format!("map {counter} = "))
        .unwrap_or_else(|| panic!("the mapping maps {counter}"))
        + format!("map {counter} = ").len();
    let rest = &mapping[start..];
    let end = rest
        .find("\n  map ")
        .or_else(|| rest.find("\n\n"))
        .expect("end");
    whitespace_collapsed(&rest[..end])
}

/// A measure replaces the original's counter and its ledger invariant: it is
/// the projection counting what the counter counted, which the design
/// draft's refinement mapping writes out (`map paid_count = count(o: Order
/// where …)`). It adds no state.
#[test]
fn measures_are_the_counts_the_draft_maps_its_counters_to() {
    let model = model(&fixture("multi_entity", "fsl5_en.fsl"));
    let projections = model
        .projections
        .iter()
        .map(|projection| {
            (
                projection.name.as_str(),
                projection.entity.as_str(),
                projection.stage.as_str(),
                whitespace_collapsed(&fsl_core::expr_text(&projection.expr)),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projections,
        vec![
            ("paid_count", "Order", "Settled", mapped_count("paid_count")),
            (
                "refund_count",
                "Order",
                "Refunded",
                mapped_count("refund_count")
            ),
        ]
    );
    assert!(
        model
            .state
            .iter()
            .all(|(name, _)| name != "paid_count" && name != "refund_count"),
        "a measure adds no state"
    );
}

/// The design draft left open whether a measure can carry a `where`
/// condition on its projection: it can. A concept's `where` is part of the
/// count, over the instance's fields and relation roles, and is type-checked
/// as the projection is.
#[test]
fn a_measure_carries_its_concepts_where_condition() {
    let source = edit(
        &fixture("multi_entity", "fsl5_en.fsl"),
        "  measure paid_count   = count Settled\n",
        "  concept LargeSettled = Order in Paid where qty > 1 and buyer != none\n  \
         measure large_paid = count LargeSettled\n  measure paid_count   = count Settled\n",
    );
    let model = model(&source);
    let large = model
        .projections
        .iter()
        .find(|projection| projection.name == "large_paid")
        .expect("the measure");
    assert_eq!(
        whitespace_collapsed(&fsl_core::expr_text(&large.expr)),
        "count(o: Order where order_stage[o] == Paid and (order_qty[o] > 1 and \
         order_buyer[o] != none))"
    );
    let (output, exit) = fslc(
        "check",
        "where-ill-typed",
        &edit(&source, "where qty > 1 and", "where qty and"),
    );
    assert_eq!((output["result"].as_str(), exit), (Some("error"), 2));
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("'large_paid'")),
        "{output:#}"
    );
}

/// A scenario step of the event that makes a relation names the instance
/// it records, as an input named by the role (`with assignee 1`); the step
/// without it is an error. The example's forbidden trace is live: without
/// the event that closes the desk, it is not rejected.
#[test]
fn a_scenario_step_names_the_instance_a_relation_records() {
    let source = fixture("relations", "fsl5_en.fsl");
    let (output, exit) = fslc("check", "scenario", &source);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
    let (output, exit) = fslc(
        "check",
        "scenario-no-role",
        &edit(
            &source,
            "    Agent takes t1 with assignee 1\n",
            "    Agent takes t1\n",
        ),
    );
    assert_eq!((output["result"].as_str(), exit), (Some("error"), 2));
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("`with assignee <number>`")),
        "{output:#}"
    );
    let (output, exit) = fslc(
        "check",
        "scenario-desk-open",
        &edit(&source, "    Agent escalates t1\n", ""),
    );
    assert_eq!(
        (output["result"].as_str(), output["kind"].as_str(), exit),
        (Some("error"), Some("forbidden"), 2),
        "{output:#}"
    );
}

/// decisions/048 J3: a guard that reads a resource is enumerated over the
/// resource's type, so the lowering itself finds the conflict. A guard that
/// reads a relation role (an `Option`) is left to the solver, which reads
/// the role's state and is asked by `kernel` as by `check`.
#[test]
fn guards_on_resources_are_enumerated_and_on_roles_go_to_the_solver() {
    // Without the traces, which `check` replays before it asks the solver.
    let source = fixture("relations", "fsl5_en.fsl");
    let traces = source.find("  scenario AC-1").expect("the scenario")
        ..source.find("}\nverify").expect("the document's end");
    let mut source = source.clone();
    source.replace_range(traces, "");
    let conflict = |guard: &str| {
        edit(
            &source,
            "    allow Agent takes Ticket only when desk_open\n",
            &format!(
                "    allow Agent takes Ticket only when {guard}\n    never Agent takes Ticket \
                 when {guard}\n"
            ),
        )
    };
    let (output, exit) = fslc("kernel", "resource-conflict", &conflict("desk_open"));
    assert_eq!(
        (output["diagnostic_code"].as_str(), exit),
        (Some("rule_conflict_unsatisfiable_guard"), 2),
        "{output:#}"
    );
    let role = conflict("assignee == none");
    let (output, exit) = fslc("kernel", "role-conflict-kernel", &role);
    assert_eq!(
        (output["diagnostic_code"].as_str(), exit),
        (Some("rule_conflict_unsatisfiable_guard"), 2),
        "{output:#}"
    );
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains("(checked by the solver, because")),
        "the enumeration leaves a role to the solver: {output:#}"
    );
    let (output, exit) = fslc("check", "role-conflict", &role);
    assert_eq!(
        (output["diagnostic_code"].as_str(), exit),
        (Some("rule_conflict_unsatisfiable_guard"), 2),
        "{output:#}"
    );
}

/// The declarations' errors, each on one edit of the multi-entity example.
#[test]
fn relation_resource_measure_and_goal_errors() {
    let source = fixture("multi_entity", "fsl5_en.fsl");
    let relation = "  Order belongs to User as buyer via place";
    for (label, old, new, expected) in [
        (
            "actor-is-not-the-target",
            relation,
            "  Order belongs to User as buyer via approve",
            "the actor of 'approve' is 'Shop'",
        ),
        (
            "target-not-an-entity",
            relation,
            "  Order belongs to Shop as buyer via place",
            "'Shop', which is not an entity",
        ),
        (
            "entity-without-lifecycle",
            relation,
            "  User belongs to User as buyer via place",
            "'User' is not an entity with a lifecycle",
        ),
        (
            "not-an-event",
            relation,
            "  Order belongs to User as buyer via buy",
            "'buy', which is not an event",
        ),
        (
            "role-is-a-field",
            relation,
            "  Order belongs to User as qty via place",
            "the role 'qty' of 'Order' is already a field",
        ),
        (
            "role-input-name-taken",
            "User places Order with qty ",
            "User places Order with qty, u: Qty ",
            "as its input 'u', which it already has",
        ),
        (
            "input-named-as-the-role",
            "User places Order with qty ",
            "User places Order with qty, buyer ",
            "input 'buyer' of event 'place' is a relation role of 'Order'",
        ),
        (
            "set-the-role",
            "    set stock = stock + qty\n",
            "    set buyer = none\n",
            "sets the relation role 'buyer'",
        ),
        (
            "set-neither-field-nor-resource",
            "    set stock = stock + qty\n",
            "    set store = 0\n",
            "'store', which is not a field of 'Order' or a resource",
        ),
        (
            "resource-spelled-like-a-field",
            "  resource stock: Qty = 2",
            "  resource stock: Qty = 2\n  resource qty: Qty = 0",
            "resource 'qty' is spelled like a field of 'Order'",
        ),
        (
            "resource-spelled-like-a-generated-name",
            "  resource stock: Qty = 2",
            "  resource stock: Qty = 2\n  resource order_buyer: Qty = 0",
            "entity 'Order' generates 'order_buyer', which is already declared",
        ),
        (
            "measure-of-an-entity",
            "count Order in Refunded",
            "count Order",
            "measure 'refund_count' counts 'Order', which is not a concept",
        ),
        (
            "measure-stage",
            "count Order in Refunded",
            "count Order in Returned",
            "'Returned' is not a stage of the lifecycle of 'Order'",
        ),
        (
            "goal-stage",
            "some Order can reach Refunded",
            "some Order can reach Returned",
            "'Returned' is not a stage of the lifecycle of 'Order'",
        ),
    ] {
        let (output, exit) = fslc("check", label, &edit(&source, old, new));
        assert_eq!(
            (output["result"].as_str(), exit),
            (Some("error"), 2),
            "{label}: {output:#}"
        );
        let message = output["message"].as_str().unwrap_or_default();
        assert!(message.contains(expected), "{label}: {message:?}");
    }
}

/// The #1808 document: `relations` between the `entity` lines and the event
/// `place`, which takes `inputs` (`""` for none).
fn place_document(relations: &str, inputs: &str) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Qty\n  actor User\n  entity User\n  entity Box\n  \
         entity Order {{\n    lifecycle Empty -> Placed\n  }}\n{relations}  event place = User \
         places Order{inputs} Empty -> Placed\n  goal Placed1 \"An order can be placed\" some \
         Order can reach Placed\n}}\nverify {{\n  values Qty = 0..1\n  instances Order = 1\n  \
         instances User = 1\n  instances Box = 1\n}}\n"
    )
}

/// #1808: an event makes at most one relation. A second relation via the
/// same event is refused at that relation with its own code, naming the
/// first; it was `generated_name_collision`, telling the author to rename an
/// input the document does not write. An authored input named like the
/// generated one keeps that code and message, and a second relation to
/// another entity keeps the actor message.
#[test]
fn an_event_makes_at_most_one_relation() {
    let buyer = "  Order belongs to User as buyer via place\n";
    let payer = "  Order belongs to User as payer via place\n";
    let buyer_ja = "  Order 所属先 User 役名 buyer 契機 place\n";
    let payer_ja = "  Order 所属先 User 役名 payer 契機 place\n";
    let second = "event 'place' already makes the relation 'buyer'; an event can make one \
                  relation, so give 'payer' its own event at 11:3";
    let rename = "event 'place' takes the 'User' that makes the relation 'buyer' as its input \
                  'u', which it already has; rename the input";
    for (label, relations, inputs, code, message) in [
        (
            "two-relations",
            format!("{buyer}{payer}"),
            "",
            "event_already_makes_relation",
            second.to_owned(),
        ),
        (
            "two-relations-ja",
            format!("{buyer_ja}{payer_ja}"),
            "",
            "event_already_makes_relation",
            second.to_owned(),
        ),
        (
            "authored-u",
            buyer.to_owned(),
            " with u: Qty",
            "generated_name_collision",
            format!("{rename} at 11:3"),
        ),
        (
            "authored-u-two-relations",
            format!("{buyer}{payer}"),
            " with u: Qty",
            "generated_name_collision",
            format!("{rename} at 12:3"),
        ),
        (
            "relation-to-another-entity",
            format!("{buyer}  Order belongs to Box as stock via place\n"),
            "",
            "FSL-PARSE",
            "'Order' belongs to 'Box' via 'place', but the actor of 'place' is 'User'; the event \
             that makes a relation is done by the related entity at 12:17"
                .to_owned(),
        ),
    ] {
        let (output, exit) = fslc("check", label, &place_document(&relations, inputs));
        assert_eq!(
            (
                output["result"].as_str(),
                output["kind"].as_str(),
                output["diagnostic_code"].as_str(),
                output["message"].as_str(),
                exit
            ),
            (
                Some("error"),
                Some("parse"),
                Some(code),
                Some(message.as_str()),
                2
            ),
            "{label}: {output:#}"
        );
    }
    let (output, exit) = fslc("check", "one-relation", &place_document(buyer, ""));
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
}

/// `policy … every E in S must eventually be T or U` is a `leadsTo` that
/// verify checks: the example's policy holds, and without `or Escalated` an
/// escalated ticket, never closed, violates it.
#[test]
fn a_policy_is_a_leads_to_that_verify_checks() {
    let source = fixture("relations", "fsl5_en.fsl");
    let verify = ["verify", "doc.fsl", "--deadlock", "ignore"];
    let (output, exit) = fslc_with(&verify, "policy", &source);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("verified"), 0),
        "{output:#}"
    );
    let narrowed = edit(
        &source,
        "must eventually be Closed or Escalated",
        "must eventually be Closed",
    );
    let (output, exit) = fslc_with(&verify, "policy-narrowed", &narrowed);
    assert_eq!(
        (
            output["result"].as_str(),
            output["violation_kind"].as_str(),
            output["invariant"].as_str(),
            exit
        ),
        (Some("violated"), Some("leadsTo"), Some("Resolved"), 1),
        "{output:#}"
    );
}

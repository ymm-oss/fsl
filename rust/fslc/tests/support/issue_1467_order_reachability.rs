// SPDX-License-Identifier: Apache-2.0

use super::{TEMPLATE, support};
use support::{Oracle, continuation, finish};

const HEADS: &str = "s.size()>0 and s.head()==1 and t.size()>0 and t.head()==0";
const PREFIX: &str = "not q.contains(Able,Able) and not q.contains(Zed,Able) and not q.contains(Zed,Zed) and not r.contains(Able,Able) and (not q.contains(Able,Zed) or r.contains(Able,Zed))";
const ZERO_MAP: &str = "z[0]==0 and z[1]==0";
const NESTED: &str = "(oo is some(inner_present_1467)) and not (inner_present_1467 is none)";
const THREE_VALUES: &str =
    "c or (Set {oo, some(none), none}.size() == 3 and Seq {oo, some(none), none}.size() == 3)";

fn lemma(row: &str) -> (&'static str, &'static str) {
    match row {
        "seq-live-prefix" => ("ReachableHeads", HEADS),
        "relation-prefix-view" => ("ReachablePrefix", PREFIX),
        "map-key-order" => ("ReachableZeroMap", ZERO_MAP),
        "nested-presence" => ("ReachableNestedPresence", NESTED),
        _ => panic!("not a reviewed reachability row: {row}"),
    }
}

fn attach(text: &str, row: &str) -> String {
    let (name, predicate) = lemma(row);
    text.replace(
        "\n}",
        &format!("\n  invariant {name} {{ {predicate} }}\n}}"),
    )
}

pub fn order_positive(text: String, row: &str, outer: &str, outcome: &str) -> String {
    if outer == "Set"
        && outcome == "pass"
        && matches!(
            row,
            "seq-live-prefix" | "relation-prefix-view" | "map-key-order"
        )
    {
        attach(&text, row)
    } else {
        text
    }
}

pub fn preservation_positive(text: String, predicate: &str) -> String {
    if predicate == THREE_VALUES {
        attach(&text, "nested-presence")
    } else {
        text
    }
}

#[test]
fn order_reachability_is_inductive_without_capture() {
    let mut errors = Vec::new();
    for (row, good, bad) in [
        ("seq-live-prefix", "s = Seq {1}", "s = Seq {0}"),
        (
            "relation-prefix-view",
            ".add(Able,Zed) forall",
            ".add(Able,Zed).add(Able,Able) forall",
        ),
        ("map-key-order", "z[k] = 0", "z[k] = 1"),
        ("nested-presence", "oo = some(some(0))", "oo = some(none)"),
    ] {
        let (name, predicate) = lemma(row);
        let alone = TEMPLATE.replace(
            "invariant Capture { $P }",
            &format!("invariant {name} {{ {predicate} }}"),
        );
        assert!(!alone.contains("Capture"));
        errors.extend(continuation(
            &format!("1467-aux-{row}-alone"),
            &alone,
            &Oracle {
                outcome: "pass",
                step: 1,
                line: 8,
                frozen: false,
                witness: None,
            },
        ));
        // Deliberately false scalar/presence fact, not a comparator/count fault.
        // Relation init remains empty; its action inserts a forbidden pair.
        let rejecting = alone.replace(good, bad).replace(name, "Capture");
        errors.extend(continuation(
            &format!("1467-aux-{row}-reject"),
            &rejecting,
            &Oracle {
                outcome: "invariant",
                step: usize::from(row == "relation-prefix-view"),
                line: 8,
                frozen: false,
                witness: None,
            },
        ));
    }
    finish(&errors);
}

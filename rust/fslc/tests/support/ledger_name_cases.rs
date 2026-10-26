// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

pub const REQUIREMENT: &str = "@requirement(\"REQ-OWNER\", \"obligation owner\")";

pub struct Case {
    pub label: &'static str,
    pub source: String,
    pub name: &'static str,
    pub result: &'static str,
    pub kind: Option<&'static str>,
    pub engine: &'static str,
    pub depth: usize,
    pub producer: &'static str,
    pub generated: Option<&'static str>,
    pub implicit_requirement: bool,
    pub action_generated: Option<&'static str>,
}

impl Case {
    pub fn key(&self, namespace: &str, annotated: bool) -> String {
        format!(
            "{}/{}/{namespace}/{}",
            self.label,
            self.result,
            if annotated {
                "requirement"
            } else {
                "unannotated"
            }
        )
    }

    pub fn annotation(&self, annotated: bool) -> &'static str {
        if !annotated {
            ""
        } else if self.implicit_requirement {
            "@requirement(\"DOMAIN-INVARIANT\", \"Counter.Safe\")"
        } else {
            REQUIREMENT
        }
    }
}

// Every row is a producer-supported obligation/result pair, not a Cartesian
// product that invents impossible key-domain violations or deadlock CTIs.
#[allow(clippy::too_many_lines)] // Keep the executable obligation catalogue together.
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut add = |label, source: String, name, result, kind, engine, depth, producer| {
        cases.push(Case {
            label,
            source,
            name,
            result,
            kind,
            engine,
            depth,
            producer,
            generated: None,
            implicit_requirement: false,
            action_generated: None,
        });
    };
    for (label, source, name, kind) in [
        (
            "invariant",
            "state { x: Int } init { x = 0 } action step() { x = x + 1 } OWNER invariant Safe { x < 1 }",
            "Safe",
            "invariant",
        ),
        (
            "trans",
            "state { x: Int } init { x = 0 } action step() { x = x + 1 } OWNER trans Stable { x <= old(x) }",
            "Stable",
            "trans",
        ),
        (
            "ensures",
            "state { x: 0..10 } init { x = 0 } OWNER action step() { requires x < 10 x = x + 1 ensures x != 6 } invariant Range { x >= 0 and x <= 10 }",
            "step",
            "ensures",
        ),
        (
            "action_partial",
            "state { x: Int, d: Int } init { x = 5 d = 1 } OWNER action dec() { requires x > 0 and x / d < 100 x = x - 1 } action zap() { requires x == 3 d = 0 } invariant NonNeg { x >= 0 }",
            "_partial_dec",
            "partial_op",
        ),
        (
            "type_bound",
            "state { x: 0..3 } init { x = 0 } OWNER action step() { x = x + 1 } invariant NonNeg { x >= 0 }",
            "_bounds_x",
            "type_bound",
        ),
    ] {
        add(
            label,
            format!("spec Subject {{ {source} }}"),
            name,
            "violated",
            Some(kind),
            "bmc",
            8,
            "rust/fslc/src/verification_output.rs:2104-2168,2687-2778",
        );
    }
    for (label, source, name, kind) in [
        (
            "invariant",
            "state { x: Int, y: Int } init { x = 0 y = 0 } action step() { requires x < 4 x = x + 1 y = y + 1 } invariant NonNeg { x >= 0 } OWNER invariant Sync { y <= 4 }",
            "Sync",
            None,
        ),
        (
            "trans",
            "state { audit: Int, y: Int } init { audit = 0 y = 0 } action step() { requires audit < -100 audit = audit + 1 y = y + 1 } OWNER trans Stable { y <= old(y) }",
            "Stable",
            None,
        ),
        (
            "ensures",
            "state { x: 0..10 } init { x = 0 } OWNER action step() { requires x < 10 x = x + 1 ensures x != 6 } invariant Range { x >= 0 and x <= 10 }",
            "step",
            Some("ensures"),
        ),
        (
            "action_partial",
            "state { x: Int, d: Int } init { x = 5 d = 1 } OWNER action dec() { requires x > 0 and x / d < 100 x = x - 1 } action zap() { requires x == 3 d = 0 } invariant NonNeg { x >= 0 }",
            "_partial_dec",
            Some("partial_op"),
        ),
        (
            "action_key_domain",
            "type K = 0..5 state { m: Map<K, Int>, cur: K, y: Int } init { forall k: K { m[k] = 0 } cur = 0 y = 0 } action advance() { requires cur < 5 cur = cur + 1 } OWNER action peek() { y = m[cur + 1] } invariant MapNonNeg { forall k: K { m[k] >= 0 } }",
            "_key_domain_peek",
            Some("key_domain"),
        ),
    ] {
        add(
            label,
            format!("spec Subject {{ {source} }}"),
            name,
            "unknown_cti",
            kind,
            "induction",
            1,
            "rust/fslc/src/verification.rs:534-608",
        );
    }
    add("bounded_leadsto", "spec Subject { state { x: 0..5 } init { x = 0 } action step() { requires x < 5 x = x + 1 } OWNER leadsTo Done { x == 1 ~> within 1 x == 4 } }".into(), "Done", "violated", Some("leadsTo"), "bmc", 4,
        "rust/fslc/src/verification_output.rs:2970-2976");
    add("ranked_leadsto", "spec Subject { state { x: Int } init { x = 0 } action inc() { requires x < 5 x = x + 1 } invariant Range { x >= 0 and x <= 5 } OWNER leadsTo Done { x < 5 ~> x == 5 decreases x } }".into(), "Done", "unknown_cti", Some("leadsTo_rank"), "induction", 3,
        "rust/fslc/src/verification.rs:692-698");
    for (label, probe) in [
        (
            "property_partial_I",
            "invariant Probe { n == 0 or q.head() >= 0 or true }",
        ),
        (
            "property_partial_T",
            "trans Probe { n == 0 or old(n) == 0 or q.head() >= 0 or true }",
        ),
        (
            "property_partial_L",
            "leadsTo Probe { n >= 1 and q.head() > 5 and false ~> n == 2 }",
        ),
    ] {
        let source = format!(
            "spec Subject {{ state {{ q: Seq<Int, 2>, n: 0..2 }} init {{ q = Seq {{}} n = 0 }} action add() {{ requires n == 0 q = q.push(1) n = 1 }} action drop() {{ requires n == 1 q = q.pop() n = 2 }} invariant EmptyAtZero {{ n != 0 or q.size() == 0 }} invariant OneAtOne {{ n != 1 or q.size() == 1 }} OWNER {probe} }}"
        );
        for (result, engine, depth, producer) in [
            (
                "violated",
                "bmc",
                3,
                "rust/fslc/src/verification_output.rs:2114-2168",
            ),
            (
                "unknown_cti",
                "induction",
                1,
                "rust/fsl-verifier/src/induction.rs:905-977,1027-1088; rust/fslc/src/verification.rs:592-608",
            ),
        ] {
            add(
                label,
                source.clone(),
                "_partial_property_Probe",
                result,
                Some("partial_op"),
                engine,
                depth,
                producer,
            );
        }
    }
    for (label, probe) in [
        (
            "property_key_domain_I",
            "invariant Probe { m[cur + 1] >= 0 or true }",
        ),
        (
            "property_key_domain_T",
            "trans Probe { m[cur + 1] >= old(cur) or true }",
        ),
        (
            "property_key_domain_L",
            "leadsTo Probe { m[cur + 1] > 5 and false ~> cur == 5 }",
        ),
    ] {
        add(
            label,
            format!(
                "spec Subject {{ type K = 0..5 state {{ m: Map<K, Int>, cur: K }} init {{ forall k: K {{ m[k] = 0 }} cur = 0 }} action advance() {{ requires cur < 5 cur = cur + 1 }} invariant MapNonNeg {{ forall k: K {{ m[k] >= 0 }} }} OWNER {probe} }}"
            ),
            "_key_domain_property_Probe",
            "unknown_cti",
            Some("key_domain"),
            "induction",
            1,
            "rust/fsl-verifier/src/induction.rs:536-590,711; rust/fslc/src/verification.rs:592-608",
        );
    }
    add("deadlock", "spec Subject { state { x: 0..1 } init { x = 0 } OWNER action blocked() { requires x == 1 x = 1 } invariant Safe { x == 0 } }".into(), "deadlock", "violated", Some("deadlock"), "bmc", 1,
        "rust/fslc/src/verification_output.rs:2934-2946");
    cases
}

#[allow(clippy::too_many_lines)] // Data rows for the same catalogue's lowering axis.
pub fn lowered_cases() -> Vec<Case> {
    let mut lowered = Vec::new();
    // Requirements accepts these kernel-shaped declarations too. Its common
    // declarations retain identity; domain below exercises the *different*
    // authored/generated identities.
    for mut case in cases() {
        case.source = case
            .source
            .replacen("spec Subject", "requirements Subject", 1);
        lowered.push(case);
    }
    let command = "command Step {} event Stepped {} decide Step { requires x < 10 emits Stepped } evolve Stepped { x = x + 1 }";
    for (label, declaration, name, generated, result, engine, depth) in [
        (
            "domain_invariant",
            "OWNER invariant Safe { x < 1 }",
            "Safe",
            "Counter_Safe",
            "violated",
            "bmc",
            2,
        ),
        (
            "domain_invariant",
            "OWNER invariant Safe { y <= 4 }",
            "Safe",
            "Counter_Safe",
            "unknown_cti",
            "induction",
            1,
        ),
    ] {
        let body = if label == "domain_invariant" && result == "unknown_cti" {
            "command Step {} event Stepped {} decide Step { requires x < 4 emits Stepped } evolve Stepped { x = x + 1 y = y + 1 }"
        } else {
            command
        };
        lowered.push(Case {
            label, source: format!("domain Subject {{ aggregate Counter {{ state {{ x: Int = 0; y: Int = 0; }} {body} {declaration} }} }}"),
            name, result, kind: if result == "violated" { Some("invariant") } else { None },
            engine, depth,
            producer: "rust/fsl-core/src/domain_lowering.rs:2909-2917; rust/fslc/src/verification.rs:97-120,592-608; rust/fslc/src/verification_output.rs:2114-2168,2687-2721",
            generated: (!generated.is_empty()).then_some(generated),
            implicit_requirement: true,
            action_generated: None,
        });
    }
    for (result, engine, depth) in [("violated", "bmc", 8), ("unknown_cti", "induction", 1)] {
        lowered.push(Case {
            label: "domain_action_partial",
            source: "domain Subject { aggregate Counter { state { x: Int = 5; d: Int = 1; } OWNER command Step {} event Stepped {} decide Step { requires x > 0 and x / d < 100 emits Stepped } evolve Stepped { x = x - 1 } command Zap {} event Zapped {} decide Zap { requires x == 3 emits Zapped } evolve Zapped { d = 0 } invariant NonNeg { x >= 0 } } }".into(),
            name: if result == "violated" { "Step" } else { "_partial_counter_step" },
            result, kind: Some("partial_op"), engine, depth,
            producer: "rust/fsl-core/src/domain_lowering.rs:2538-2557; rust/fslc/src/verification.rs:123-143,534-608; rust/fslc/src/verification_output.rs:2104-2168",
            generated: (result == "violated").then_some("_partial_counter_step"),
            implicit_requirement: false,
            action_generated: Some("counter_step"),
        });
    }
    for (label, source, name, kind, generated, result, engine, depth) in [
        (
            "branched_ensures",
            "state { x: 0..10 } init { x = 0 } OWNER action step() { requires x < 10 branches { when true { x = x + 1 } maps stutter } ensures x != 6 } invariant Range { x >= 0 and x <= 10 }",
            "step.b1",
            "ensures",
            "step.b1",
            "unknown_cti",
            "induction",
            1,
        ),
        (
            "branched_ensures",
            "state { x: 0..10 } init { x = 0 } OWNER action step() { requires x < 10 branches { when true { x = x + 1 } maps stutter } ensures x != 6 } invariant Range { x >= 0 and x <= 10 }",
            "step.b1",
            "ensures",
            "step.b1",
            "violated",
            "bmc",
            8,
        ),
        (
            "branched_action_partial",
            "state { x: Int, d: Int } init { x = 5 d = 1 } OWNER action step() { requires x > 0 and x / d < 100 branches { when true { x = x - 1 } maps stutter } } action zap() { requires x == 3 d = 0 } invariant NonNeg { x >= 0 }",
            "_partial_step.b1",
            "partial_op",
            "step.b1",
            "unknown_cti",
            "induction",
            1,
        ),
        (
            "branched_action_partial",
            "state { x: Int, d: Int } init { x = 5 d = 1 } OWNER action step() { requires x > 0 and x / d < 100 branches { when true { x = x - 1 } maps stutter } } action zap() { requires x == 3 d = 0 } invariant NonNeg { x >= 0 }",
            "step",
            "partial_op",
            "step.b1",
            "violated",
            "bmc",
            8,
        ),
        (
            "branched_action_key_domain",
            "type K = 0..5 state { m: Map<K, Int>, cur: K, y: Int } init { forall k: K { m[k] = 0 } cur = 0 y = 0 } action advance() { requires cur < 5 cur = cur + 1 } OWNER action peek() { branches { when true { y = m[cur + 1] } maps stutter } } invariant MapNonNeg { forall k: K { m[k] >= 0 } }",
            "_key_domain_peek.b1",
            "key_domain",
            "peek.b1",
            "unknown_cti",
            "induction",
            1,
        ),
        (
            "branched_type_bound",
            "state { x: 0..3 } init { x = 0 } OWNER action step() { branches { when true { x = x + 1 } maps stutter } } invariant NonNeg { x >= 0 }",
            "_bounds_x",
            "type_bound",
            "step.b1",
            "violated",
            "bmc",
            8,
        ),
    ] {
        lowered.push(Case {
            label, source: format!("requirements Subject {{ {source} }}"), name, result, kind: Some(kind), engine, depth,
            producer: "rust/fsl-core/src/dialect.rs:1792-1858,1903-1951; rust/fslc/src/verification.rs:123-143,534-608; rust/fslc/src/verification_output.rs:2104-2168,2687-2778",
            generated: None, implicit_requirement: false, action_generated: Some(generated),
        });
    }
    lowered
}

pub fn public_name(name: &str, composed: bool) -> String {
    if !composed || name == "deadlock" {
        return name.to_owned();
    }
    for prefix in [
        "_partial_property_",
        "_key_domain_property_",
        "_partial_",
        "_key_domain_",
        "_bounds_",
    ] {
        if let Some(suffix) = name.strip_prefix(prefix) {
            return format!("{prefix}ab.{suffix}");
        }
    }
    format!("ab.{name}")
}

pub fn assert_ledger(body: &str, case: &Case, name: &str, annotated: bool) {
    let id = if annotated && case.label != "deadlock" {
        if case.implicit_requirement {
            "DOMAIN-INVARIANT"
        } else {
            "REQ-OWNER"
        }
    } else {
        "（仕様全体）"
    };
    let detail = super::findings::assert_issue(body, id, &[name]);
    if case.result == "unknown_cti" {
        super::findings::assert_uncertain(detail);
    }
    if annotated && case.label != "deadlock" {
        assert!(
            !super::findings::section(body, "リスク一覧（要件ID別）").contains("| （仕様全体） |"),
            "unexpected spec-level attribution"
        );
    }
    if annotated && case.label == "deadlock" {
        assert!(
            super::findings::risk_row(body, "REQ-OWNER").contains("🟢"),
            "deadlock must not blame the blocked action"
        );
    }
}

pub fn collect_cell(
    green: &mut Vec<String>,
    red: &mut Vec<String>,
    key: String,
    check: impl FnOnce() + std::panic::UnwindSafe,
) {
    if std::panic::catch_unwind(check).is_ok() {
        green.push(key);
    } else {
        red.push(key);
    }
}

pub fn finish_matrix(green: &[String], red: &[String]) {
    println!("MATRIX GREEN {}: {}", green.len(), green.join(", "));
    println!("MATRIX RED {}: {}", red.len(), red.join(", "));
    assert!(
        red.is_empty(),
        "incorrect attribution combinations: {red:#?}"
    );
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1580: helpful arguments resolve constants and bare enum members, not state.

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod compose_scratch;

use compose_scratch::{run, scratch};

const HINT: &str = "helpful action arguments must be state-independent leadsto binders, constants, enum members, or arithmetic (+, -, *) over those values";

const INTEGER: &str = "spec A {
  const ONE = 1
  type Id = 0..1
  state { n: Id }
  init { n = 1 }
  fair action flip(i: Id) { n = 0 }
  leadsTo Progress { n > 0 ~> n == 0 helpful flip(ONE) decreases n }
}
";

fn outcomes(source: &str, state_dependent: bool) {
    let dir = scratch("issue-1580-helpful", &[("a.fsl", source)]);
    let (checked, code) = run(&dir, &["check", "a.fsl"]);
    eprintln!("check exit={code} output={checked}");
    assert_eq!(
        (code, checked["result"].as_str()),
        (0, Some("ok")),
        "{checked}"
    );
    for engine in ["bmc", "induction"] {
        let (output, code) = run(
            &dir,
            &[
                "verify",
                "a.fsl",
                "--engine",
                engine,
                "--depth",
                "3",
                "--no-cache",
            ],
        );
        eprintln!("{engine} exit={code} output={output}");
        let refused = state_dependent && engine == "induction";
        let expected = match (state_dependent, engine) {
            (true, "induction") => (2, "error"),
            (_, "bmc") => (0, "verified"),
            _ => (0, "proved"),
        };
        assert_eq!(
            (code, output["result"].as_str()),
            (expected.0, Some(expected.1)),
            "{engine}: {output}"
        );
        if refused {
            assert_eq!(output["kind"], "semantics", "{output}");
            let message = output["message"].as_str().expect("diagnostic message");
            assert!(message.contains(HINT), "{output}");
            assert!(message.contains("unknown name 'n'"), "{output}");
        }
    }
}

#[test]
fn integer_constant_helpful_argument_proves() {
    // ONE is a const, disjoint from state n (also in the negative fixture).
    outcomes(INTEGER, false);
}

#[test]
fn binder_and_constant_arithmetic_helpful_argument_proves() {
    // ONE is disjoint from state level and binder c; exercise +, -, and *.
    outcomes(
        "spec P {
  const ONE = 1
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }
  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  leadsTo Progress {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c + ONE * ONE - ONE)
    decreases level[c]
  }
}
",
        false,
    );
}

#[test]
fn boolean_constant_helpful_argument_proves() {
    // ON is a const, disjoint from state n.
    outcomes(
        "spec B {
  const ON = true
  state { n: 0..1 }
  init { n = 1 }
  fair action flip(on: Bool) { n = 0 }
  leadsTo Progress { n > 0 ~> n == 0 helpful flip(ON) decreases n }
}
",
        false,
    );
}

#[test]
fn bare_enum_member_helpful_argument_proves() {
    // Low and High are enum members, disjoint from state n.
    outcomes(
        "spec E {
  enum Level { Low, High }
  state { n: 0..1 }
  init { n = 1 }
  fair action flip(l: Level) { n = 0 }
  leadsTo Progress { n > 0 ~> n == 0 helpful flip(Low) decreases n }
}
",
        false,
    );
}

#[test]
fn preservation_state_dependent_helpful_argument_is_refused() {
    let source = INTEGER.replace("helpful flip(ONE)", "helpful flip(n)");
    outcomes(&source, true);
}

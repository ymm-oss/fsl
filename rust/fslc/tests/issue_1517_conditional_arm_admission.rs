// SPDX-License-Identifier: Apache-2.0

//! #1517 names a scalar conditional's supplied type in its diagnostics. That must
//! not change admission: `Int` inside a container makes compatibility
//! non-transitive, so a container conditional keeps its mutual arm check.
//! The f1-f4 packets are the ones the base before #1517 produced; the scalar
//! rejection differs from that base only in naming the supplied type.

#[path = "support/issue_1517_scratch.rs"]
mod scratch;

use serde_json::{Value, json};
use std::{fs, process::Command};

const ARM_NARROW: &str = "spec ArmNarrow { type A = 0..1 type B = 0..2\n  state { c: Bool, a: Set<A>, b: Set<B> }\n  init { c = false a = Set {} b = Set {} }\n  action flip() {\n    let t = Set { 0 }\n    requires a == (if c then t else b)\n    c = not c\n  }\n}\n";
const ARM_WIDEN: &str = "spec ArmWiden { type A = 0..1 type B = 0..2\n  state { c: Bool, a: Set<A>, b: Set<B> }\n  init { c = false a = Set {} b = Set {} }\n  action flip() { c = not c }\n  action addA() { a = a.add(0) }\n  invariant P { not (Set { 0 } == (if c then a else b)) }\n}\n";
const ARM_SEQ: &str = "spec ArmSeq { type A = 0..1\n  state { c: Bool, q: Seq<Int, 2>, a: Seq<A, 2>, b: Seq<A, 3> }\n  init { c = false q = Seq {} a = Seq {} b = Seq {} }\n  action flip() { c = not c }\n  action grow() { requires b.size() < 3\n b = b.push(0) }\n  invariant P { q == (if c then a else b) }\n}\n";
const ARM_MAP: &str = "spec ArmMap { type A = 0..1 type B = 0..2 type K = 0..1\n  state { c: Bool, m0: Map<K, Int>, m1: Map<K, A>, m2: Map<K, B> }\n  init { c = false forall k: K { m0[k] = 0 m1[k] = 0 m2[k] = 0 } }\n  action flip() { c = not c }\n  invariant P { m0 == (if c then m1 else m2) }\n}\n";

fn direct(statement: &str) -> String {
    format!(
        "spec Direct {{\n  type A = 0..1\n  state {{ c: Bool, d: Bool, m: Map<A, Bool> }}\n  init {{ c = false d = false forall k: A {{ m[k] = false }} }}\n  action go() {{\n    {statement}\n  }}\n}}\n"
    )
}

fn check(id: &str, source: &str) -> (Value, i32) {
    let scratch = scratch::Scratch::new(
        "FSLC_1517_CONDITIONAL_INPUTS",
        id,
        &format!("conditional-{id}"),
    );
    fs::write(scratch.path().join("p.fsl"), source).expect("fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "p.fsl"])
        .current_dir(scratch.path())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("CLI");
    assert!(
        output.stderr.is_empty(),
        "{id}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        serde_json::from_slice(&output.stdout).expect("JSON packet"),
        output.status.code().expect("exit"),
    )
}

fn versions() -> Value {
    fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    )
}

fn assert_rejected(id: &str, source: &str, reason: &str, line: u32, column: u32) {
    assert_eq!(
        check(id, source),
        (
            json!({"fsl":"1.0","result":"error","kind":"semantics",
                "message":format!("invalid model expression: {reason} at {line}:{column}"),
                "loc":{"line":line,"column":column},"versions":versions()}),
            2
        ),
        "{id}"
    );
}

#[test]
fn f1_set_arm_narrower_than_its_sibling_stays_admitted() {
    assert_eq!(
        check("f1", ARM_NARROW),
        (
            json!({"fsl":"1.0","result":"ok","spec":"ArmNarrow","warnings":[{
                "kind":"no_user_invariants",
                "message":"spec declares no user invariants (only implicit type bounds are checked)"}],
                "versions":versions()}),
            0
        )
    );
}

#[test]
fn f2_set_arms_with_different_carriers_stay_rejected() {
    assert_rejected(
        "f2",
        ARM_WIDEN,
        "expression of type Set(Named(\"B\")) is not assignable to Set(Named(\"A\"))",
        6,
        53,
    );
}

#[test]
fn f3_seq_arms_with_different_capacities_stay_rejected() {
    assert_rejected(
        "f3",
        ARM_SEQ,
        "expression of type Seq(Named(\"A\"), 3) is not assignable to Seq(Named(\"A\"), 2)",
        7,
        40,
    );
}

#[test]
fn f4_map_arms_with_different_carriers_stay_rejected() {
    assert_rejected(
        "f4",
        ARM_MAP,
        "expression of type Map(Named(\"K\"), Named(\"B\")) is not assignable to Map(Named(\"K\"), Named(\"A\"))",
        5,
        42,
    );
}

#[test]
fn scalar_read_key_arm_names_the_supplied_map_key_type() {
    assert_rejected(
        "scalar_read",
        &direct("d = m[if c then 0 else true]"),
        "expression of type Bool is not assignable to Named(\"A\")",
        6,
        28,
    );
}

#[test]
fn scalar_read_key_arms_stay_admitted() {
    let (packet, exit) = check("scalar_valid", &direct("d = m[if c then 0 else 1]"));
    assert_eq!((exit, &packet["result"]), (0, &json!("ok")), "{packet}");
}

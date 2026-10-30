// SPDX-License-Identifier: Apache-2.0

//! None-valued lexical binders retain type context without changing Set support.

#[allow(dead_code)]
#[path = "support/collection_replay.rs"]
mod collection_replay;
use collection_replay::{ReplayCase, finish, require};

#[test]
fn none_bindings_preserve_verdicts_and_existing_set_restrictions() {
    let mut errors = Vec::new();
    for (family, binder, left, right) in [
        ("seq-none-left", "x in Seq {some(n), none}", "x", "o"),
        ("seq-none-right", "x in Seq {some(n), none}", "o", "x"),
        ("seq-some-only", "x in Seq {some(n)}", "x", "o"),
        ("typed", "k: K", "some(k)", "o"),
        ("set-none-left", "x in Set {some(n), none}", "x", "o"),
        ("set-none-right", "x in Set {some(n), none}", "o", "x"),
    ] {
        for tautology in [false, true] {
            let predicate = if tautology {
                format!("forall {binder}: {left} == {right} or {left} != {right}")
            } else {
                format!("forall {binder}: {left} == {right}")
            };
            let source = format!(
                "spec NoneBinderContext {{\n type K = 0..1\n state {{ n: K, o: Option<K> }}\n init {{ n = 0 o = none }}\n action go() {{ n = n }}\n invariant Capture {{ {predicate} }}\n}}\n"
            );
            for repeat in 0..2 {
                let outcome = if tautology { "tautology" } else { "negative" };
                let name = format!("{family}-{outcome}-{repeat}");
                let case = ReplayCase::new(&name, &source, &["go"]);
                for route in ["check", "bmc", "induction", "explicit", "monitor"] {
                    let (r, exit) = case.observe(route, 4);
                    let agrees = match route {
                        "check" => exit == 0 && r["result"] == "ok",
                        "monitor" if tautology => {
                            exit == 0 && r["result"] == "conformant" && r["steps_checked"] == 1
                        }
                        "monitor" => {
                            exit == 1
                                && r["result"] == "nonconformant"
                                && r["violation"]["kind"] == "invariant"
                                && r["violation"]["name"] == "Capture"
                                && r["failed_at_event"] == 0
                        }
                        _ if family.starts_with("set-") => {
                            exit == 2
                                && r["result"] == "error"
                                && r["kind"] == "semantics"
                                && r["message"] == "Set literal binder requires scalar elements"
                        }
                        _ if tautology => {
                            exit == 0
                                && r["result"] == if route == "bmc" { "verified" } else { "proved" }
                                && r["completeness"]
                                    == if route == "bmc" {
                                        "bounded"
                                    } else {
                                        "unbounded"
                                    }
                        }
                        _ => {
                            exit == 1
                                && r["result"] == "violated"
                                && r["violation_kind"] == "invariant"
                                && r["invariant"] == "Capture"
                                && r["violated_at_step"] == 0
                        }
                    };
                    require(&mut errors, agrees, &name, route, &r, exit);
                }
            }
        }
    }
    finish(&errors);
}

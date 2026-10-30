// SPDX-License-Identifier: Apache-2.0

#[path = "support/collection_replay.rs"]
mod collection_replay;
use collection_replay::{ReplayCase, finish, require};

#[test]
fn equality_supplies_context_before_constructing_either_operand() {
    let mut errors = Vec::new();
    for kind in ["Seq", "Set"] {
        let state = if kind == "Seq" { "Seq<N, 2>" } else { "Set<N>" };
        for contents in ["", "2"] {
            let literal = format!("{kind} {{{contents}}}");
            for literal_first in [true, false] {
                let conditional = if literal_first {
                    format!("if not c then {literal} else ys")
                } else {
                    format!("if c then ys else {literal}")
                };
                for conditional_left in [true, false] {
                    let predicate = if conditional_left {
                        format!("({conditional}) == ys")
                    } else {
                        format!("ys == ({conditional})")
                    };
                    let source = format!(
                        "spec EmptyContext {{\n  type N = 0..2\n  state {{ c: Bool, ys: {state} }}\n  init {{ c = true ys = {kind} {{1}} }}\n  action go() {{ c = false }}\n  invariant Capture {{ {predicate} }}\n}}\n"
                    );
                    for _ in 0..2 {
                        let case = ReplayCase::new("empty-context", &source, &["go"]);
                        for route in ["check", "bmc", "induction", "explicit", "monitor"] {
                            let (r, exit) = case.observe(route, 4);
                            let agrees = match route {
                                "check" => exit == 0 && r["result"] == "ok",
                                "monitor" => {
                                    exit == 1
                                        && r["result"] == "nonconformant"
                                        && r["violation"]["kind"] == "invariant"
                                        && r["violation"]["name"] == "Capture"
                                        && r["failed_at_event"] == 0
                                }
                                _ => {
                                    exit == 1
                                        && r["result"] == "violated"
                                        && r["violation_kind"] == "invariant"
                                        && r["invariant"] == "Capture"
                                        && r["violated_at_step"] == 1
                                }
                            };
                            require(&mut errors, agrees, &predicate, route, &r, exit);
                        }
                    }
                }
            }
        }
    }
    finish(&errors);
}

// SPDX-License-Identifier: Apache-2.0
//! #1480: a form `check` accepts never reaches an engine's range refusal.
//!
//! The static rule and the engines answer the same question from different inputs,
//! so this grid is the gate on their drift: scope by source shape by state Set write.
//! Controls stay in the grid, and each listed family pins its own verdict.
use serde_json::Value;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn run(source: &str, args: &[&str]) -> (i32, Value) {
    let dir = std::env::temp_dir().join(format!(
        "issue-1480-agreement-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.fsl"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .arg("m.fsl")
        .current_dir(&dir)
        .env("FSLC_CACHE", "off")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.code().unwrap_or(-1), value)
}

fn check(source: &str) -> (i32, Value) {
    run(source, &["check"])
}

fn verify(source: &str, engine: &str, extra: &[&str]) -> (i32, Value) {
    let mut args = vec!["verify", "--engine", engine, "--depth", "3", "--no-cache"];
    args.extend_from_slice(extra);
    run(source, &args)
}

fn result(run: &(i32, Value)) -> &str {
    run.1["result"].as_str().unwrap_or("<no json>")
}

const WRITES: &[&str] = &[
    "b = true",
    "xs = xs.add(n)",
    "xs = Set { 1, 2 } ys = xs",
    "xs = xs.remove(n) ys = ys.add(1)",
    "xs = Set { n } n = 0",
    "if b { xs = ys } else { xs = xs.add(2) }",
    "xs = xs.add(n) ys = xs",
    "ys = xs xs = xs.add(n)",
];

const INITS: &[&str] = &[
    "xs = Set { 1 } ys = Set { }",
    "xs = Set { 0, 2 } ys = xs",
    "xs = Set { n } ys = Set { y }",
    "xs = Set { n } ys = xs",
];

const SOURCES: &[&str] = &[
    "forall e in xs { count(j in 0..e) >= 0 }",
    "forall e in ys { forall j in 0..e { j >= 0 } }",
    "sum(e in xs of count(j in 0..e)) >= 0",
    "count(e in xs.add(1) where count(j in 0..e) >= 0) >= 0",
    "forall e in xs.remove(n) { count(j in 0..e) >= 0 }",
    "forall e in Set { 0, 1 } { count(j in 0..e) >= 0 }",
    "forall e in Set { n } { count(j in 0..e) >= 0 }",
    "forall e in Seq { 0, 1 } { count(j in 0..e) >= 0 }",
    "forall e in q { count(j in 0..e) >= 0 }",
    "forall i in 0..n { i >= 0 }",
    "forall a in 0..2 { forall c in 0..y { c >= 0 } }",
    "(o is some(v)) and count(j in 0..v) >= 0",
];

fn scoped(scope: &str, source: &str, write: &str) -> String {
    match scope {
        "invariant" => format!("action go() {{ {write} }} invariant P {{ {source} }}"),
        "trans" => format!("action go() {{ {write} }} trans P {{ {source} }}"),
        "reachable" => format!("action go() {{ {write} }} reachable P {{ ({source}) or true }}"),
        "requires" => format!("action go() {{ requires {source} {write} }}"),
        "ensures" => format!("action go() {{ {write} ensures {source} }}"),
        "terminal" => format!("action go() {{ requires not b b = true }} terminal {{ {source} }}"),
        "fair" => format!(
            "fair action go() {{ requires {source} {write} }} leadsTo L {{ b ~> within 2 true }}"
        ),
        "leadsP" => {
            format!("fair action go() {{ {write} }} leadsTo L {{ ({source}) ~> within 2 true }}")
        }
        "wrapped" => format!(
            "fair action go() {{ {write} }} leadsTo L {{ forall w in 0..1 {{ ({source}) ~> within 2 true }} }}"
        ),
        other => panic!("unknown scope {other}"),
    }
}

const SCOPES: &[&str] = &[
    "invariant",
    "trans",
    "reachable",
    "requires",
    "ensures",
    "terminal",
    "fair",
    "leadsP",
    "wrapped",
];

fn refused(run: &(i32, Value)) -> bool {
    run.1["message"].as_str().is_some_and(|message| {
        message.contains("[planner]")
            || message.contains("[eligibility]")
            || message.contains("is not an integer const")
    })
}

/// Every `check`-accepted cell runs on BMC and induction, and both classes stay inhabited.
#[test]
fn a_checked_form_never_reaches_an_engine_range_refusal() {
    let (mut accepted, mut rejected) = (0, 0);
    let mut drift = Vec::new();
    for init in INITS {
        for write in WRITES {
            for source in SOURCES {
                for scope in SCOPES {
                    let item = scoped(scope, source, write);
                    let form = format!(
                        "spec M {{ type T = 0..3 state {{ n: T, y: T, b: Bool, o: Option<T>, \
                         xs: Set<T>, ys: Set<T>, q: Seq<T, 2> }} init {{ n = 1 y = 0 b = false \
                         o = some(1) q = Seq {{ 0 }} {init} }} {item} }}"
                    );
                    if check(&form).0 != 0 {
                        rejected += 1;
                        continue;
                    }
                    accepted += 1;
                    for engine in ["bmc", "induction"] {
                        let run = verify(&form, engine, &[]);
                        if refused(&run) {
                            drift.push(format!("{engine}: {form}: {}", run.1["message"]));
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "AGREEMENT accepted={accepted} rejected={rejected} drift={}",
        drift.len()
    );
    assert!(drift.is_empty(), "{}", drift.join("\n"));
    assert!(accepted > 500, "accepted={accepted}");
    assert!(rejected > 500, "rejected={rejected}");
}

const PREFIX: &str = "spec M { type T = 0..3 ";

fn form(body: &str) -> String {
    format!("{PREFIX}{body} }}")
}

/// Each row is a family where `check` accepted and an engine refused.
#[test]
fn every_family_agrees() {
    // (body, check exit, then bmc and induction results when it is accepted)
    let rows: &[(&str, &str, i32, &str, &str)] = &[
        (
            "fair guard",
            "state { n: T, y: T } init { n = 0 y = 0 } fair action w() { requires y == 0 and count(i in 0..n) == 1 y = 1 } leadsTo L { y == 0 ~> y == 1 }",
            0,
            "verified",
            "proved",
        ),
        (
            "old-frame read",
            "state { n: T } init { n = 0 } action inc() { requires n < 3 n = n + 1 } trans P { forall i in 0..n { i % 2 >= 0 } }",
            0,
            "verified",
            "proved",
        ),
        (
            "state Set",
            "state { n: T, st: Set<T> } init { n = 0 st = Set { } } action a() { requires n < 2 st = st.add(n) n = n + 1 } invariant P { forall e in st { forall j in 0..e { j <= 1 } } }",
            0,
            "verified",
            "proved",
        ),
        (
            "reachable diagnosis",
            "state { n: T, y: T } init { n = 0 y = 0 } action inc() { y = 0 } reachable R { y == 1 } invariant B { forall i in 0..n { i >= 0 } }",
            0,
            "reachable_failed",
            "reachable_failed",
        ),
        (
            "static outer binder",
            "state { n: T, y: T } init { n = 0 y = 0 } action inc() { requires n < 2 n = n + 1 } invariant A { forall a in 0..2 { forall b in 0..y { b >= 0 } } }",
            0,
            "verified",
            "proved",
        ),
        (
            "non-empty init literal",
            "state { st: Set<T>, z: Bool } init { st = Set { 1, 2 } z = forall e in st { forall j in 0..e { j >= 0 } } } action a() { z = z } invariant P { z }",
            0,
            "verified",
            "proved",
        ),
        (
            "let rebound by a later binder",
            "state { n: T, y: T } init { n = 0 y = 0 } action go() { let c = Seq { n } y = 1 requires forall n in 0..1 { forall e in c { forall j in 0..e { j >= 0 } } } }",
            2,
            "",
            "",
        ),
        (
            "folded init condition the engine joins",
            "state { n: T, y: T, z: Bool } init { z = (1 < 2) if z { n = 1 } else { n = 9 } y = count(i in 0..n) } action a() { y = y } invariant P { y == 2 }",
            2,
            "",
            "",
        ),
    ];
    for repeat in 0..2 {
        for &(family, body, exit, bmc, induction) in rows {
            let source = form(body);
            let checked = check(&source);
            assert_eq!(checked.0, exit, "{repeat} {family}: {}", checked.1);
            for (engine, expected) in [("bmc", bmc), ("induction", induction)] {
                let run = verify(&source, engine, &[]);
                if exit == 0 {
                    assert_eq!(result(&run), expected, "{family} {engine}: {}", run.1);
                } else {
                    // The refusal `check` reports is the one every engine reports.
                    assert_eq!(run.1["message"], checked.1["message"], "{family} {engine}");
                    assert_eq!(run.1["loc"], checked.1["loc"], "{family} {engine}");
                    assert_eq!(run.0, 2);
                }
            }
        }
    }
}

/// A Set whose key is read from state stays refused where no frame certifies it.
#[test]
fn a_symbolic_key_is_opaque_without_a_certified_frame() {
    let set = |write: &str, scope: &str| {
        form(&format!(
            "state {{ n: T, st: Set<T> }} init {{ n = 0 st = Set {{ }} }} action a() {{ {write} }} {scope}"
        ))
    };
    let body = "forall e in st { count(j in 0..e) >= 0 }";
    let reach = format!("reachable R {{ {body} }}");
    let inv = format!("invariant R {{ {body} }}");
    for (write, literal) in [
        ("st = st.add(n)", false),
        ("st = Set { n }", false),
        ("st = st.add(1)", true),
        ("st = Set { 1, 2 }", true),
        ("st = st.remove(n)", true),
    ] {
        let reached = check(&set(write, &reach));
        assert_eq!(reached.0 == 0, literal, "{write}: {}", reached.1);
        // The carrier is certified here whatever the key was.
        assert_eq!(check(&set(write, &inv)).0, 0, "{write}");
    }
}

/// Only init's writes and an action's own expression give a Set a key the engines cannot
/// order. A copy made in init, or read from an action's frame, runs in all three engines.
#[test]
fn a_copy_carries_a_key_only_in_init() {
    const STATE: &str = "state { n: T, y: T, b: Bool, xs: Set<T>, ys: Set<T> }";
    const ENSURES: &str = "ensures forall e in ys { forall j in 0..e { j >= 0 } }";
    const REACHES: &str = "reachable R { forall e in ys { count(j in 0..e) >= 0 } }";
    let copied = "init { n = 1 y = 0 b = true xs = Set { 0, 2 } ys = xs }";
    let spec = |init: &str, body: &str| form(&format!("{STATE} {init} {body}"));
    // ca66ca948d7c: the init copy, then a later write to the Set it copied.
    for body in [
        format!("action go() {{ xs = xs.add(n) b = true {ENSURES} }}"),
        format!("action go() {{ xs = xs.add(n) ys = ys.add(1) {ENSURES} }}"),
        format!("action go() {{ ys = xs xs = xs.add(n) b = true {ENSURES} }}"),
        format!("action go() {{ xs = xs.add(n) if b {{ ys = xs }} b = false {ENSURES} }}"),
        format!("action go() {{ xs = xs.add(n) b = true }} action h() {{ ys = xs {ENSURES} }}"),
        format!("action go() {{ xs = xs.add(n) b = true }} {REACHES}"),
        format!("action go() {{ xs = xs.add(n) ys = xs }} {REACHES}"),
        // `old` reads the frame the action started from, before its own write.
        "action go() { xs = xs.add(n) ensures forall e in old(xs) { forall j in 0..e { j >= 0 } } }"
            .to_owned(),
    ] {
        let source = spec(copied, &body);
        assert_eq!(check(&source).0, 0, "{source}: {}", check(&source).1);
        assert_eq!(result(&verify(&source, "bmc", &[])), "verified", "{source}");
        assert_eq!(
            result(&verify(&source, "induction", &[])),
            "proved",
            "{source}"
        );
        assert_eq!(
            result(&verify(&source, "explicit", &[])),
            "proved",
            "{source}"
        );
    }
    // Writing the key directly still leaves it symbolic in the next state.
    for body in [
        format!("action go() {{ ys = ys.add(n) {ENSURES} }}"),
        format!("action go() {{ ys = xs.add(n) {ENSURES} }}"),
        format!("action go() {{ let s = xs.add(n) ys = s {ENSURES} }}"),
        format!("action go() {{ b = true {ENSURES} }} action h() {{ ys = Set {{ n }} }}"),
        format!("action go() {{ ys = ys.add(n) }} {REACHES}"),
    ] {
        let source = spec(copied, &body);
        let refused = check(&source);
        assert_eq!(refused.0, 2, "{source}: {}", refused.1);
        assert!(
            refused.1["message"]
                .as_str()
                .unwrap()
                .contains("'e' is not an integer const"),
            "{source}: {}",
            refused.1
        );
    }
    // An initial key that is not a literal reaches an initial state and no postcondition.
    let initial = "init { y = 0 n = y b = true xs = Set { n } ys = xs }";
    let reaches = spec(initial, &format!("action go() {{ b = true }} {REACHES}"));
    assert_eq!(check(&reaches).0, 2, "{}", check(&reaches).1);
    let postcondition = spec(initial, &format!("action go() {{ b = true {ENSURES} }}"));
    assert_eq!(check(&postcondition).0, 0, "{}", check(&postcondition).1);
    assert_eq!(result(&verify(&postcondition, "bmc", &[])), "verified");
    assert_eq!(result(&verify(&postcondition, "induction", &[])), "proved");
}

/// The engines read `old(...)` from one frame and have none below it. A range there is
/// refused at `check`, as the base refused it, and not left for every engine to stop on.
#[test]
fn a_range_under_old_with_no_frame_left_is_refused_at_check() {
    const STATE: &str =
        "state { n: T, y: T, b: Bool, xs: Set<T> } init { n = 1 y = 0 b = true xs = Set { 0, 2 } }";
    let spec = |body: &str| form(&format!("{STATE} {body}"));
    for (body, name) in [
        (
            "action go() { b = true } unless U { (forall x in old(xs) { count(y in 0..x) >= 0 }) unless b }",
            "x",
        ),
        (
            "action go() { b = true } unless U { (old(count(i in 0..n) >= 0)) unless b }",
            "n",
        ),
        (
            "action go() { b = true } invariant W { old(count(i in 0..n) >= 0) }",
            "n",
        ),
        (
            "action go() { b = true } trans W { old(old(count(i in 0..n) >= 0)) }",
            "n",
        ),
        (
            "action go() { requires old(count(i in 0..n) >= 0) b = true }",
            "n",
        ),
        (
            "action go() { let g = old(count(i in 0..n) >= 0) b = true ensures g }",
            "n",
        ),
        (
            "action go(k: T) { let g = old(count(i in 0..k) >= 0) b = true ensures g }",
            "k",
        ),
        (
            "action go() { b = true } invariant W { old(forall x in 0..2 { count(j in 0..x) >= 0 }) }",
            "x",
        ),
    ] {
        let source = spec(body);
        let refused = check(&source);
        assert_eq!(refused.0, 2, "{body}: {}", refused.1);
        let message = refused.1["message"].as_str().unwrap();
        assert!(
            message.starts_with(&format!(
                "invalid model expression: '{name}' is not an integer const at "
            )),
            "{body}: {message}"
        );
    }
    // One `old` with a frame stays admitted, and every engine runs it.
    for body in [
        "action go() { b = true } trans W { old(count(i in 0..n)) >= 0 }",
        "action go() { b = true } trans W { forall x in old(xs) { count(y in 0..x) >= 0 } }",
        "action go() { b = true } trans W { old(forall x in 0..2 { count(j in 0..x) >= 0 }) }",
        "action go() { b = true } trans W { forall x in xs { old(count(j in 0..x) >= 0) } }",
        "action go() { b = true } unless U { (forall x in xs { count(y in 0..x) >= 0 }) unless b }",
        "action go() { b = true } unless U { (count(y in 0..n) >= 0) unless b }",
    ] {
        let source = spec(body);
        assert_eq!(check(&source).0, 0, "{body}: {}", check(&source).1);
        assert_eq!(result(&verify(&source, "bmc", &[])), "verified", "{body}");
        assert_eq!(
            result(&verify(&source, "induction", &[])),
            "proved",
            "{body}"
        );
        assert_eq!(
            result(&verify(&source, "explicit", &[])),
            "proved",
            "{body}"
        );
    }
    // An `old` no range reads under stays with the engines, on the base as well.
    for body in [
        "action go() { b = true } unless U { old(b) unless b }",
        "action go() { b = true } unless U { (forall e in old(xs) { e >= 0 }) unless b }",
        "action go() { b = true } invariant W { old(count(i in 0..2) >= 0) }",
    ] {
        let source = spec(body);
        assert_eq!(check(&source).0, 0, "{body}: {}", check(&source).1);
        for engine in ["bmc", "induction", "explicit"] {
            let stopped = verify(&source, engine, &[]);
            assert_eq!(stopped.0, 2, "{body} {engine}: {}", stopped.1);
            assert_eq!(
                stopped.1["message"], "old() used without old state",
                "{body} {engine}"
            );
        }
    }
}

/// `--property` no longer refuses a range whose frame proves the unselected bound.
#[test]
fn a_selected_property_proves_the_bound_it_reads() {
    let proved = form(
        "state { n: T } init { n = 0 } action inc() { requires n < 3 n = n + 1 } invariant P { forall i in 0..n { i <= 3 } } invariant Q { n <= 3 }",
    );
    for property in ["P", "Q"] {
        let bmc = verify(&proved, "bmc", &["--property", property]);
        assert_eq!(result(&bmc), "verified", "{property}: {}", bmc.1);
    }
    let guarded = form(
        "state { n: T, y: T } init { n = 0 y = 0 } action inc() { requires count(i in 0..n) >= 0 and n < 3 n = n + 1 } invariant Q { y == 0 }",
    );
    assert_eq!(
        result(&verify(&guarded, "bmc", &["--property", "Q"])),
        "verified"
    );
    // The frame cannot prove the escaped bound, so the read refuses instead of clipping.
    let escaped = form(
        "state { n: T, y: T } init { n = 3 y = 0 } action inc() { n = n + 1 } invariant P { forall i in 0..n { i <= 3 } } invariant Q { y == 0 }",
    );
    let guard_read = form(
        "state { n: T, y: T } init { n = 3 y = 0 } action inc() { requires count(i in 0..n) >= 0 n = n + 1 } invariant Q { y == 0 }",
    );
    for (source, property) in [(&escaped, "P"), (&guard_read, "Q")] {
        let refused = verify(source, "bmc", &["--property", property]);
        assert_eq!(refused.0, 2, "{source}: {}", refused.1);
        assert!(
            refused.1["message"]
                .as_str()
                .unwrap()
                .contains("[eligibility]"),
            "{source}: {}",
            refused.1
        );
    }
}

/// A read whose carrier bound only the unselected properties would establish. The state
/// past the escape is where the selected property fails, so the explicit engine reports it
/// and bmc must refuse the read: assuming the bound would prune that state and prove `Q`.
#[test]
fn a_read_past_an_unselected_bound_is_never_assumed_in_bounds() {
    const SPEC: &str = "state { n: T, y: Int, b: Bool } init { n = 3 y = 0 b = false }";
    let selected = &["--property", "Q"][..];
    for body in [
        // The action's own read, reached only by the unrolling after the checked step.
        "action inc() { y = if count(i in 0..n) == 5 then 1 else 0 n = n + 1 } invariant Q { y == 0 }",
        "action inc() { if count(i in 0..n) == 5 { y = 1 } n = n + 1 } invariant Q { y == 0 }",
        // The selected property's read of the frame before the step.
        "action inc() { n = n + 1 y = y + 1 } trans Q { old(count(i in 0..n)) <= 4 }",
        "action inc() { n = n + 1 y = y + 1 } trans Q { old(forall i in 0..n { i <= 3 }) }",
    ] {
        let source = form(&format!("{SPEC} {body}"));
        assert_eq!(check(&source).0, 0, "{body}: {}", check(&source).1);
        let truth = verify(&source, "explicit", selected);
        assert_eq!(result(&truth), "violated", "{body}: {}", truth.1);
        let refused = verify(&source, "bmc", selected);
        assert_eq!(refused.0, 2, "{body}: {}", refused.1);
        assert!(
            refused.1["message"]
                .as_str()
                .unwrap()
                .contains("[eligibility]"),
            "{body}: {}",
            refused.1
        );
        // With the bound selected, the escape is the finding.
        let whole = verify(&source, "bmc", &[]);
        assert_eq!(result(&whole), "violated", "{body}: {}", whole.1);
        assert_eq!(
            whole.1["violation_kind"], "type_bound",
            "{body}: {}",
            whole.1
        );
    }
    // No state escapes, so the frame proves the bound and the same reads run.
    for body in [
        "action inc() { requires n < 3 y = if count(i in 0..n) == 5 then 1 else 0 n = n + 1 } invariant Q { y == 0 }",
        "action inc() { requires n < 3 n = n + 1 } trans Q { old(count(i in 0..n)) <= 4 }",
    ] {
        let source = form(&format!(
            "state {{ n: T, y: Int, b: Bool }} init {{ n = 0 y = 0 b = false }} {body}"
        ));
        let run = verify(&source, "bmc", selected);
        assert_eq!(result(&run), "verified", "{body}: {}", run.1);
    }
}

#[test]
fn the_plain_named_bound_reports_its_real_violation() {
    let source = "spec M { const n = 0 type T = 0..3 state { n: T } init { n = 1 } action a() { n = n } invariant P { forall i in 0..n { i == 0 } } }";
    assert_eq!(check(source).0, 0);
    for engine in ["bmc", "induction", "explicit"] {
        assert_eq!(result(&verify(source, engine, &[])), "violated", "{engine}");
    }
}

#[test]
fn a_declared_type_named_bool_is_refused() {
    for declaration in ["enum Bool { A, B, C }", "type Bool = 0..2"] {
        let source = format!(
            "spec M {{ {declaration} state {{ n: 0..1 }} init {{ n = 0 }} action a() {{ n = n }} }}"
        );
        let refused = check(&source);
        assert_eq!(refused.0, 2, "{}", refused.1);
        assert!(
            refused.1["message"]
                .as_str()
                .unwrap()
                .contains("reserved FSL type name"),
            "{}",
            refused.1
        );
    }
    let control = "spec M { enum Boolean { A, B } state { n: 0..1 } init { n = 0 } action a() { n = n } invariant P { forall b: Boolean { b != A or b != B } } }";
    assert_eq!(check(control).0, 0);
    // The builtin stays enumerable.
    let builtin = "spec M { state { n: 0..1 } init { n = 0 } action a() { n = n } invariant P { forall b: Bool { b or not b } } }";
    assert_eq!(result(&verify(builtin, "bmc", &[])), "verified");
}

#[test]
fn an_unbounded_typed_binder_is_refused_at_check() {
    for item in [
        "invariant P { forall i: Int { i == i } }",
        "fair action f() { n = n } leadsTo L { forall i: Int { n == 0 ~> within 2 n == 0 } }",
    ] {
        let source = format!(
            "spec M {{ state {{ n: 0..1 }} init {{ n = 0 }} action a() {{ n = n }} {item} }}"
        );
        let refused = check(&source);
        assert_eq!(refused.0, 2, "{item}: {}", refused.1);
        assert_eq!(
            refused.1["message"], "type 'Int' is not a finite scalar",
            "{item}"
        );
        assert!(refused.1["loc"].is_object(), "{item}: {}", refused.1);
    }
    let finite = "spec M { type K = 0..1 state { n: 0..1 } init { n = 0 } action a() { n = n } invariant P { forall i: K { i == i } } }";
    assert_eq!(check(finite).0, 0);
}

/// A Kernel projection failing on an aggregate bound still names where it is.
#[test]
fn projections_locate_the_aggregate_bound_they_cannot_fold() {
    let source = form(
        "state { n: T, y: T } init { n = 0 y = 0 } action a() { requires count(i in 0..n) >= 0 y = 1 }",
    );
    let at = source.find("action a()").unwrap() + 1;
    for command in ["kernel", "typestate"] {
        let (exit, output) = run(&source, &[command]);
        assert_eq!(exit, 2, "{command}: {output}");
        assert_eq!(output["kind"], "semantics", "{command}");
        assert_eq!(
            output["message"],
            format!("invalid model expression: 'n' is not an integer const at 1:{at}"),
            "{command}"
        );
        assert_eq!(output["loc"]["column"], at, "{command}");
    }
    let closed =
        form("state { n: T } init { n = 0 } action a() { requires count(i in 0..2) >= 0 n = 1 }");
    assert_eq!(run(&closed, &["kernel"]).0, 0);
}

/// Native half of the Worker's `issue_1480_governance_refuses_an_unexecutable_preserved_side`.
#[test]
fn a_preserved_side_the_engines_cannot_execute_is_refused() {
    let dir = std::env::temp_dir().join(format!("issue-1480-governance-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, source) in [
        (
            "after.fsl",
            "spec SmallImpl { type IQty = 0..3 state { n: IQty } init { n = 0 } action bump() { requires n < 3  n = n + 1 } reachable R { forall i in 0..n { i >= 0 } } }\n",
        ),
        (
            "small_abs.fsl",
            "spec SmallAbs { type AQty = 0..3 state { n: AQty } init { n = 0 } action bump() { requires n < 3  n = n + 1 } }\n",
        ),
        (
            "map.fsl",
            "refinement M { impl SmallImpl abs SmallAbs maps auto }\n",
        ),
        (
            "governance.fsl",
            "governance WideControls {\n  control CTRL-WIDE \"The sequence is preserved\"\n\n  preservation WidePreserved {\n    before SmallAbs from \"small_abs.fsl\"\n    after SmallImpl from \"after.fsl\"\n    preserve CTRL-WIDE\n    checked_by refinement \"map.fsl\"\n  }\n}\n",
        ),
    ] {
        std::fs::write(dir.join(name), source).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "governance.fsl"])
        .current_dir(&dir)
        .env("FSLC_CACHE", "off")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let native: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(2), "{native}");
    assert_eq!(native["kind"], "type", "{native}");
    assert_eq!(native["message"], "'n' is not an integer const", "{native}");
    assert_eq!(
        native["loc"],
        serde_json::json!({"line": 4, "column": 3}),
        "{native}"
    );
}

#[cfg(target_os = "linux")]
mod limited {
    use super::*;

    fn limited(source: &str, args: &str) -> (i32, Value, Vec<u8>) {
        let dir = std::env::temp_dir().join(format!(
            "issue-1480-limited-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.fsl"), source).unwrap();
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "ulimit -v 3000000; exec '{}' {args}",
                env!("CARGO_BIN_EXE_fslc")
            ))
            .current_dir(&dir)
            .env("FSLC_CACHE", "off")
            .output()
            .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        (
            output.status.code().unwrap_or(-1),
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
            output.stderr,
        )
    }

    /// Admission does not enumerate a wrapper's product, however wide.
    #[test]
    fn admission_of_a_wide_wrapper_stays_within_memory() {
        let source = "spec WideL { state { y: Bool } init { y = false } action a() { y = true } leadsTo L { forall k in 0..100000000 { forall j in 0..k { y ~> within 2 y } } } }";
        let (exit, output, _) = limited(source, "check m.fsl");
        assert_eq!(
            (exit, &output["result"]),
            (0, &Value::from("ok")),
            "{output}"
        );
    }

    /// A range too wide for memory refuses in JSON instead of panicking, wherever it runs
    /// out: reserving the candidate list, enumerating, or building terms over candidates.
    #[test]
    fn enumeration_out_of_memory_is_a_refusal() {
        for domain in ["1000000000", "12000000", "300000"] {
            for body in [
                "action a() { n = 1 } invariant P { forall i in 0..n { i >= 0 } }",
                "action a() { requires forall i in 0..n { i >= 0 } n = 1 }",
            ] {
                let source = format!(
                    "spec Z {{ type W = 0..{domain} state {{ n: W }} init {{ n = 1 }} {body} }}"
                );
                for engine in ["bmc", "induction"] {
                    let (exit, output, _) = limited(
                        &source,
                        &format!("verify m.fsl --engine {engine} --depth 2 --no-cache"),
                    );
                    assert_eq!(exit, 2, "{domain} {engine}: {output}");
                    assert!(
                        output["message"]
                            .as_str()
                            .unwrap_or("")
                            .contains("[resource]"),
                        "{domain} {engine}: {output}"
                    );
                }
            }
        }
    }
}

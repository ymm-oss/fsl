// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1595: pulled progress retains abstract constant/binder scope or rejects capture.
//! These are verdict contracts, not envelope parity: refine reports implementation
//! traces and progress metadata, whereas BMC reports abstract traces and assurance.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);

impl Fixture {
    fn new(impl_const: &str, abs_const: &str, property: &str) -> Self {
        let implementation = format!(
            "spec Impl {{ {impl_const} state {{ x: Int = 0 }}
              fair action go() {{ requires x == 0 x = 1 }} }}"
        );
        let abstraction = format!(
            "spec Abs {{ {abs_const} state {{ y: Int = 0 }}
              fair action step() {{ requires y == 0 y = 1 }}
              leadsTo L {{ {property} }} }}"
        );
        Self::sources(
            &implementation,
            &abstraction,
            "refinement R { impl Impl abs Abs map y = x action go() -> step()
                  preserve progress { respond L by go } }",
        )
    }

    fn sources(implementation: &str, abstraction: &str, mapping: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "fslc-1595-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create #1595 fixture directory");
        for (file, source) in [
            ("impl.fsl", implementation),
            ("abs.fsl", abstraction),
            ("map.fsl", mapping),
        ] {
            std::fs::write(root.join(file), source).expect("write #1595 fixture");
        }
        Self(root)
    }

    fn observations(&self, cell: &str) -> Vec<((Value, i32), (Value, i32))> {
        (0..2)
            .map(|_| (self.observe(cell, "refine"), self.observe(cell, "bmc")))
            .collect()
    }

    fn observe(&self, cell: &str, consumer: &str) -> (Value, i32) {
        let args = if consumer == "refine" {
            vec!["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"]
        } else {
            vec![
                "verify",
                "abs.fsl",
                "--engine",
                "bmc",
                "--depth",
                "4",
                "--no-cache",
            ]
        };
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("run native fslc");
        for stream in [&output.stdout, &output.stderr] {
            assert!(
                !String::from_utf8_lossy(stream).contains("_substitution_"),
                "{cell}/{consumer}: internal name leaked: {output:?}"
            );
        }
        let actual: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{cell}/{consumer}: invalid JSON: {error}; {output:?}"));
        let exit = output.status.code().expect("native exit code");
        eprintln!("OBS cell={cell} consumer={consumer} exit={exit} output={actual}");
        assert!(output.stderr.is_empty(), "{cell}/{consumer}: {output:?}");
        (actual, exit)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned #1595 fixture");
    }
}

enum Expected {
    Lost(Value),
    Located(&'static str),
    StaticBound,
}
use Expected::{Located, Lost, StaticBound};

fn assert_error(actual: &Value, exit: i32, kind: &str) {
    assert_eq!(exit, 2, "{actual:#}");
    assert_eq!(actual["result"], "error", "{actual:#}");
    assert_eq!(actual["kind"], kind, "{actual:#}");
}

fn assert_lost(actual: &Value, exit: i32, result: &str, bindings: &Value) {
    assert_eq!(exit, 1, "{actual:#}");
    assert_eq!(actual["result"], result, "{actual:#}");
    assert_eq!(actual["invariant"], "L", "{actual:#}");
    assert_eq!(actual["violation_kind"], "leadsTo", "{actual:#}");
    assert_eq!(&actual["bindings"], bindings, "{actual:#}");
}

fn run(cell: &str, impl_const: &str, abs_const: &str, property: &str, expected: &Expected) {
    let fixture = Fixture::new(impl_const, abs_const, property);
    let observations = fixture.observations(cell);
    let static_bound = "'N' is not an integer const";
    for (_, (abs, exit)) in &observations {
        if matches!(expected, StaticBound | Located("N")) {
            assert_error(abs, *exit, "semantics");
            assert_eq!(abs["message"], static_bound, "{abs:#}");
        } else {
            let bindings = match expected {
                Lost(bindings) => bindings.clone(),
                Located(name) => json!({*name: 2}),
                StaticBound => unreachable!(),
            };
            assert_lost(abs, *exit, "violated", &bindings);
        }
    }
    for ((actual, exit), (abs, abs_exit)) in &observations {
        match expected {
            Lost(bindings) => {
                assert_lost(actual, *exit, "refinement_failed", bindings);
                assert_eq!(actual["kind"], "progress_lost", "{actual:#}");
                assert_eq!(actual["bindings"], abs["bindings"]);
                assert_eq!(exit, abs_exit);
            }
            Located(name) => {
                assert_error(actual, *exit, "type");
                assert!(actual["loc"].is_object(), "{actual:#}");
                let message = actual["message"].as_str().expect("located error message");
                assert!(message.contains(name), "{actual:#}");
                if *name == "N" {
                    assert_eq!(exit, abs_exit);
                }
            }
            StaticBound => {
                assert_error(actual, *exit, "semantics");
                assert_eq!(actual["message"], static_bound, "{actual:#}");
                assert_eq!(actual["message"], abs["message"]);
                assert_eq!(exit, abs_exit);
            }
        }
    }
}

macro_rules! cells {
    ($($name:ident: ($ic:expr, $ac:expr, $property:expr, $expected:expr)),* $(,)?) => {
        $(#[test]
        fn $name() {
            run(stringify!($name), $ic, $ac, $property, &$expected);
        })*
    };
}

cells! {
    f1: ("const N = 1", "const N = 3",
        "forall z in 0..N { y == 0 ~> z <= 1 }", Lost(json!({"z": 2}))),
    f2: ("const N = 1", "",
        "forall N in 0..3 { forall z in 0..N { y == 0 ~> z <= 1 } }", Located("N")),
    c1: ("const M = 1", "const N = 3",
        "forall z in 0..N { y == 0 ~> z <= 1 }", Lost(json!({"z": 2}))),
    s1c: ("const M = 1", "const N = 3", "y == 0 ~> y >= N", Lost(json!({}))),
    c2: ("const M = 1", "",
        "forall N in 0..3 { forall z in 0..N { y == 0 ~> z <= 1 } }", StaticBound),
    c3: ("const N = 1", "",
        "forall K in 0..3 { forall z in 0..3 { (y == 0 and z <= K) ~> z <= 1 } }",
        Lost(json!({"K": 2, "z": 2}))),
    s1: ("const N = 1", "const N = 3", "y == 0 ~> y >= N", Lost(json!({}))),
    s2: ("", "const x = 3", "y == 0 ~> y >= x", Lost(json!({}))),
    s5: ("const N = 1", "const N = 3",
        "y == 0 ~> (forall k in 0..N { k <= 1 })", Lost(json!({}))),
    s4: ("", "", "forall y in 2..3 { y == 2 ~> y == 3 }", Lost(json!({"y": 2}))),
    s3: ("", "", "forall x in 2..3 { y == 0 ~> y == x }", Located("x")),
    s3c: ("", "", "forall w in 2..3 { y == 0 ~> y == w }", Lost(json!({"w": 2}))),
    s4c: ("", "", "forall w in 2..3 { w == 2 ~> w == 3 }", Lost(json!({"w": 2}))),
    r3_bool_const: ("const B=true", "const B=false", "y==0 ~> B", Lost(json!({}))),
}

type ReviewCase<'a> = (i32, Option<i32>, Option<&'a str>, bool, &'a Value);
fn review(cell: &str, src: (&str, &str, &str), case: ReviewCase<'_>) {
    let (implementation, abstraction, maps) = src;
    let (expected, abs_exit, message, input, bindings) = case;
    let mapping = format!(
        "refinement R {{ impl Impl abs Abs {maps} preserve progress {{ respond L by go }} }}"
    );
    let fixture = Fixture::sources(implementation, abstraction, &mapping);
    for ((actual, exit), (mut abs, abstract_exit)) in fixture.observations(cell) {
        if let Some(expected) = abs_exit {
            assert_eq!(abstract_exit, expected);
            let expected = usize::try_from(expected).unwrap();
            assert_eq!(abs["result"], ["verified", "violated", "error"][expected]);
        }
        assert_eq!(exit, expected, "{actual:#}");
        if let Some(message) = message {
            assert_eq!(actual["message"], message, "{actual:#}");
        }
        match expected {
            2 => {
                let kind = if input { "semantics" } else { "type" };
                assert_error(&actual, exit, kind);
                if input {
                    assert!(abs.as_object_mut().unwrap().remove("versions").is_some());
                    assert_eq!(actual, abs);
                } else {
                    assert_eq!(
                        actual["loc"],
                        json!({"line": 1, "column": mapping.find("respond").unwrap() + 1})
                    );
                }
            }
            1 => {
                assert_lost(&actual, exit, "refinement_failed", bindings);
                assert_eq!(actual["kind"], "progress_lost");
                if abs_exit == Some(1) {
                    assert_eq!(actual["bindings"], abs["bindings"]);
                }
            }
            0 => assert_eq!(actual["result"], "refines"),
            _ => unreachable!(),
        }
    }
}

macro_rules! review_cells {
    ($($name:ident: ($i:expr, $a:expr, $m:expr, $exit:expr, $abs:expr, $message:expr, $input:expr, $bindings:expr)),* $(,)?) => {
        $(#[test] fn $name() { review(stringify!($name), ($i, $a, $m), ($exit, $abs, $message, $input, &$bindings)); })*
    };
}
const SCALAR_IMPL: &str = "spec Impl{state{x:Int=0}fair action go(){requires x==0 x=1}}";
const SCALAR_MAP: &str = "map y=x action go()->step()";
const PATTERN_CAPTURE: &str = "rename the abstract pattern binder 'x' to avoid mapped-name capture";
const MAPPING_CAPTURE: &str = "rename the mapping pattern binder 'z' to avoid capture";
review_cells! {
    r1_pattern_capture: ("spec Impl { type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1} }",
        "spec Abs { type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> ((o is some(x)) and y==x)} }",
        "map y=x map o=p action go()->step()", 2, Some(1), Some(PATTERN_CAPTURE), false, json!({})),
    r1_pattern_or: ("spec Impl { type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1} }",
        "spec Abs { type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> ((o is some(x)) or y==3)} }",
        "map y=x map o=p action go()->step()", 2, Some(0), Some(PATTERN_CAPTURE), false, json!({})),
    r1_pattern_const: ("spec Impl { type V=0..5 const N=1 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1} }",
        "spec Abs { type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> ((o is some(N)) and (forall k in 0..N {k<=1}))} }",
        "map y=x map o=p action go()->step()", 2, Some(2), None, false, json!({})),
    r1_ambiguous_const_state: ("spec Impl {state {x:Int=0} fair action go(){x=0}}",
        "spec Abs {const N=3 state {N:Int=1} fair action step(){N=1} leadsTo L {forall z in 0..N {true ~> z<=1}}}",
        "map N=1 action go()->step()", 2, None, None, false, json!({})),
    r1_unused_map: ("spec Impl {state {x:Int=0} fair action go(){requires x==0 x=1}}",
        "spec Abs {state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {forall x in 0..3 {x==0 ~> x==0}}}",
        "map y=x action go()->step()", 0, Some(0), None, false, json!({})),
    r1_nested_const_binder: ("spec Impl {const k=1 state {x:Int=0} fair action go(){requires x==0 x=1}}",
        "spec Abs {state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> count(k in 0..3 where k>=0)<=2}}",
        "map y=x action go()->step()", 2, Some(1), None, false, json!({})),
    r1_indexed_key_const: ("spec Impl {type K=0..1 const Z=1 state {d:Map<K,Int>} init {forall k:K {d[k]=0}} fair action go(k:K){requires d[k]==0 and k==1 d[k]=1}}",
        "spec Abs {type K=0..1 const Z=0 state {m:Map<K,Int>} init {forall k:K {m[k]=0}} fair action step(k:K){requires m[k]==0 m[k]=1} leadsTo L {m[Z]==0 ~> m[Z]==1}}",
        "map m[k:K]=d[k] action go(k)->step(k)", 1, Some(0), None, false, json!({})),
    r1_indexed_own_binder: ("spec Impl {type K=0..1 state {d:Map<K,Int>} init {forall k:K {d[k]=0}} fair action go(k:K){requires d[k]==0 d[k]=1}}",
        "spec Abs {type K=0..1 state {m:Map<K,Int>} init {forall k:K {m[k]=0}} fair action step(k:K){requires m[k]==0 m[k]=1} leadsTo L {forall k:K {m[k]==0 ~> m[k]==1}}}",
        "map m[k:K]=d[k] action go(k)->step(k)", 0, Some(0), None, false, json!({})),
    r2_pattern_and_false_or: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> (((o is some(x)) and false) or y==3)}}",
        "map y=x map o=p action go()->step()", 2, Some(1), Some(PATTERN_CAPTURE), false, json!({})),
    r2_pattern_or_false_and: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> (((o is some(x)) or false) and y==3)}}",
        "map y=x map o=p action go()->step()", 2, Some(1), Some(PATTERN_CAPTURE), false, json!({})),
    r2_pattern_not_or: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> (not (o is some(x)) or y==3)}}",
        "map y=x map o=p action go()->step()", 2, Some(1), Some(PATTERN_CAPTURE), false, json!({})),
    r2_pattern_implies_or: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> (((o is some(x)) => false) or y==3)}}",
        "map y=x map o=p action go()->step()", 2, Some(1), Some(PATTERN_CAPTURE), false, json!({})),
    r2_pattern_fresh: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> (((o is some(w)) or false) and y==3)}}",
        "map y=x map o=p action go()->step()", 1, Some(1), None, false, json!({})),
    r2_nested_shadow: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {true ~> (forall y in 0..0 {(o is some(x)) and y==0})}}",
        "map y=x map o=p action go()->step()", 0, Some(0), None, false, json!({})),
    r2_indexed_shadow: ("spec Impl {type V=0..5 type K=0..1 state {x:Int=0,p:Option<V> = some(3),d:Map<K,Int>} init {forall k:K {d[k]=0}} fair action go(){requires x==0 x=0}}",
        "spec Abs {type V=0..5 type K=0..1 state {y:Int=0,o:Option<V> = some(3),m:Map<K,Int>} init {forall k:K {m[k]=0}} fair action step(){requires y==0 y=0} leadsTo L {true ~> (forall m in 0..0 {(o is some(x)) and m==0})}}",
        "map y=x map o=p map m[k:K]=d[k]+x action go()->step()", 0, Some(0), None, false, json!({})),
    r2_unused_local: (SCALAR_IMPL, "spec Abs {const N=3 state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {forall N in 0..3 {y==0 ~> y==1}}}", SCALAR_MAP, 2, Some(2), Some("invalid model expression: local binding 'N' shadows a constant of the same name; rename the local binding at 1:75"), true, json!({})),
    r2_unused_local_fresh: ("spec Impl {state {x:Int=0} fair action go(){requires x==0 x=1}}",
        "spec Abs {const N=3 state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {forall M in 0..3 {y==0 ~> y==1}}}",
        "map y=x action go()->step()", 0, Some(0), None, false, json!({})),
    r2_local_read: (SCALAR_IMPL, "spec Abs {const N=3 state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {forall N in 0..1 {N==0 ~> N==0}}}", SCALAR_MAP, 2, Some(2), None, true, json!({})),
    r2_local_domain_only: (SCALAR_IMPL, "spec Abs {const N=3 state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {forall N in 0..N {y==0 ~> y==1}}}", SCALAR_MAP, 2, Some(2), None, true, json!({})),
    r2_local_filter_read: (SCALAR_IMPL, "spec Abs {const N=3 state {y:Int=0} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> count(N in 0..1 where N>=0)<=2}}", SCALAR_MAP, 2, Some(2), None, true, json!({})),
    r3_map_property: ("spec Impl {type V=0..5 state {p:Option<V> = some(1)} fair action go(){p=some(1)}}",
        "spec Abs {state {b:Bool=false} fair action step(){b=false} leadsTo L {forall z in 0..3 {true ~> (b or z!=3)}}}",
        "map b=((p is some(z)) and z==3) action go()->step()", 2, Some(1), Some(MAPPING_CAPTURE), false, json!({})),
    r3_map_property_fresh: ("spec Impl {type V=0..5 state {p:Option<V> = some(1)} fair action go(){p=some(1)}}",
        "spec Abs {state {b:Bool=false} fair action step(){b=false} leadsTo L {forall z in 0..3 {true ~> (b or z!=3)}}}",
        "map b=((p is some(w)) and w==3) action go()->step()", 1, Some(1), None, false, json!({"z":3})),
    r3_map_sibling: ("spec Impl {type V=0..5 state {p:Option<V> = some(1),q:Option<V> = some(3)} fair action go(){p=some(1) q=some(3)}}",
        "spec Abs {state {b:Bool=false,c:Bool=true} fair action step(){b=false c=true} leadsTo L {true ~> (b or not c)}}",
        "map b=((p is some(z)) and z==3) map c=((q is some(z)) and z==3) action go()->step()", 2, Some(1), Some(MAPPING_CAPTURE), false, json!({})),
    r3_map_sibling_fresh: ("spec Impl {type V=0..5 state {p:Option<V> = some(1),q:Option<V> = some(3)} fair action go(){p=some(1) q=some(3)}}",
        "spec Abs {state {b:Bool=false,c:Bool=true} fair action step(){b=false c=true} leadsTo L {true ~> (b or not c)}}",
        "map b=((p is some(z)) and z==3) map c=((q is some(w)) and w==3) action go()->step()", 1, Some(1), None, false, json!({})),
    r3_map_unused: ("spec Impl {type V=0..5 state {p:Option<V> = some(1)} fair action go(){p=some(1)}}",
        "spec Abs {state {b:Bool=false} fair action step(){b=false} leadsTo L {forall z in 0..3 {true ~> true}}}",
        "map b=((p is some(z)) and z==3) action go()->step()", 0, Some(0), None, false, json!({})),
    r3_pattern_before: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {((o is some(x)) and y==x) ~> false}}",
        "map y=x map o=p action go()->step()", 2, Some(0), Some(PATTERN_CAPTURE), false, json!({})),
    r3_pattern_nested: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> (forall k in 0..1 {(o is some(x)) and y==x})}}",
        "map y=x map o=p action go()->step()", 2, Some(1), Some(PATTERN_CAPTURE), false, json!({})),
    r3_pattern_decreases: ("spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}",
        "spec Abs {type V=0..5 state {y:Int=0,o:Option<V> = some(3)} fair action step(){requires y==0 y=1} leadsTo L {y==0 ~> y==1 decreases if (o is some(x)) then y else 0}}",
        "map y=x map o=p action go()->step()", 2, Some(0), Some(PATTERN_CAPTURE), false, json!({})),
    r3_pattern_indexed: ("spec Impl {type V=0..5 type K=0..1 state {x:Int=0,p:Option<V> = some(3),d:Map<K,Int>} init {forall k:K {d[k]=0}} fair action go(){x=0}}",
        "spec Abs {type V=0..5 type K=0..1 state {o:Option<V> = some(3),m:Map<K,Int>} init {forall k:K {m[k]=0}} fair action step(){m[0]=0} leadsTo L {true ~> ((o is some(x)) and m[0]==0)}}",
        "map o=p map m[k:K]=d[k]+x action go()->step()", 2, Some(0), Some(PATTERN_CAPTURE), false, json!({})),
    r3_map_property_pattern: ("spec Impl {type V=0..5 state {p:Option<V> = some(1),q:Option<V> = some(3)} fair action go(){p=some(1) q=some(3)}}",
        "spec Abs {type V=0..5 state {b:Bool=false,o:Option<V> = some(3)} fair action step(){b=false o=some(3)} leadsTo L {true ~> ((o is some(z)) and b)}}",
        "map b=((p is some(z)) and z==3) map o=q action go()->step()", 2, Some(1), Some(MAPPING_CAPTURE), false, json!({})),
    r3_map_aggregate: ("spec Impl {type V=0..5 state {p:Option<V> = some(1)} fair action go(){p=some(1)}}",
        "spec Abs {state {b:Bool=false} fair action step(){b=false} leadsTo L {true ~> count(z in 0..3 where b)<=0}}",
        "map b=((p is some(z)) and z==3) action go()->step()", 2, Some(0), Some(MAPPING_CAPTURE), false, json!({})),
}

// SPDX-License-Identifier: Apache-2.0

//! Every implicit bound assumed by induction must also be checked in its base.
//! Collection negatives require #1541 retention; never trim them to fit main.

#[path = "support/report_cli.rs"]
mod report_cli;
#[path = "support/scratch_identity.rs"]
mod scratch_identity;

use serde_json::{Value, json};

const G02: &str = r"spec G02 {
  type V = 0..3
  type C = 0..5
  state { v: V, c: C, flag: Bool }
  init { v = 7 - 2 c = 0 flag = false }
  action tick() { requires c < 5 c = c + 1 }
  action look() { requires c == 5 flag = v == 5 }
  invariant P { not flag }
}";

const G04: &str = r"spec G04 {
  type V = 0..3
  type C = 0..5
  state { v: V, c: C, flag: Bool }
  init { v = 0 c = 0 flag = false }
  action escape() { requires c == 0 v = 7 - 2 c = 1 }
  action tick() { requires c > 0 and c < 5 c = c + 1 }
  action look() { requires c == 5 flag = v == 5 }
  invariant P { not flag }
}";

const G01: &str = r"spec G01 {
  type K = 0..1
  type C = 0..5
  state { s: Set<K>, c: C, x: Int, flag: Bool }
  init { x = 2 s = Set { x } c = 0 flag = false }
  action tick() { requires c < 5 c = c + 1 }
  action look() { requires c == 5 flag = s.contains(2) }
  invariant P { not flag }
}";

const SET_LATER: &str = r"spec SetLater {
  type K = 0..1
  type C = 0..5
  state { s: Set<K>, c: C, x: Int, flag: Bool }
  init { x = 2 s = Set {} c = 0 flag = false }
  action escape() { requires c == 0 s = s.add(x) c = 1 }
  action tick() { requires c > 0 and c < 5 c = c + 1 }
  action look() { requires c == 5 flag = s.contains(2) }
  invariant P { not flag }
}";

const REL_INIT: &str = r"spec RelInit {
  type K = 0..1
  type C = 0..5
  state { empty: relation K -> K, r: relation K -> K, c: C, x: Int, flag: Bool }
  init { x = 2 empty = Set {} r = empty.add(x, 0) c = 0 flag = false }
  action tick() { requires c < 5 c = c + 1 }
  action look() { requires c == 5 flag = r.contains(2, 0) }
  invariant P { not flag }
}";

const REL_LATER: &str = r"spec RelLater {
  type K = 0..1
  type C = 0..5
  state { r: relation K -> K, c: C, x: Int, flag: Bool }
  init { x = 2 r = Set {} c = 0 flag = false }
  action escape() { requires c == 0 r = r.add(x, 0) c = 1 }
  action tick() { requires c > 0 and c < 5 c = c + 1 }
  action look() { requires c == 5 flag = r.contains(2, 0) }
  invariant P { not flag }
}";

const G03: &str = r"spec G03 {
  type V = 0..3
  state { v: V, flag: Bool }
  init { v = 0 flag = false }
  action look() { flag = v == 5 }
  invariant P { not flag }
}";

const COLLECTION_VALID: &str = r"spec CollectionValid {
  type K = 0..1
  state { s: Set<K>, r: relation K -> K, flag: Bool }
  init { s = Set {} r = Set {} flag = false }
  action fill() { s = s.add(1) r = r.add(1, 0) }
  action look() { flag = s.contains(2) or r.contains(2, 0) }
  invariant P { not flag }
}";

const FREE_INIT: &str = r"spec S2 {
  type V = 0..3
  state { v: V, w: Int, flag: Bool }
  init { flag = false }
  action inc() { requires v < 3 v = v + 1 }
  invariant P { v >= 0 and v <= 3 }
}";

const SIBLING: &str = r"spec S1 {
  type V = 0..3
  state { v: V, flag: Bool }
  init { v = 0 flag = true }
  action inc() { requires v < 3 v = v + 1 }
  invariant P { v <= 3 }
  invariant Q { not flag }
}";

fn cell(source: &str, bound: Option<(&str, usize)>, engine: &str) {
    let target = report_cli::repository_root().join("rust/target");
    std::fs::create_dir_all(&target).expect("fixture parent directory");
    // Each repetition gets fresh evidence; cache state cannot decide a control.
    for _ in 0..2 {
        let dir = scratch_identity::unique(target.join("issue-1598"));
        std::fs::create_dir(&dir).expect("fresh fixture directory");
        let path = dir.join("model.fsl");
        std::fs::write(&path, source).expect("write fixture");
        let depth = if engine == "induction" { 4 } else { 8 };
        let (value, exit) = report_cli::run_json(
            "verify",
            path.to_str().expect("UTF-8 path"),
            depth,
            &["--engine", engine, "--property", "P"],
        );
        if source == FREE_INIT && engine == "explicit" {
            // BFS requires a concrete init; it must not invent the free values.
            assert_eq!(
                (exit, &value["result"], &value["kind"], &value["message"]),
                (
                    2,
                    &json!("error"),
                    &json!("semantics"),
                    &json!("init does not assign state variable(s): v, w")
                ),
                "{value}"
            );
            continue;
        }
        let (verdict, completeness, expected_exit) = if bound.is_some() {
            ("violated", "bounded", 1)
        } else if engine == "bmc" {
            ("verified", "bounded", 0)
        } else {
            ("proved", "unbounded", 0)
        };
        assert_eq!(
            (exit, &value["result"], &value["completeness"]),
            (expected_exit, &json!(verdict), &json!(completeness)),
            "{engine} depth={depth} fixture={} {value}",
            path.display()
        );
        if let Some((name, step)) = bound {
            // Standalone selected BMC/BFS still check only P. Six actions are
            // needed to expose the invalid root to P, beyond the shallow base.
            let expected = if engine == "induction" {
                ("type_bound", name, step)
            } else {
                ("invariant", "P", if source == FREE_INIT { 0 } else { 6 })
            };
            assert_eq!(
                (
                    &value["violation_kind"],
                    &value["invariant"],
                    &value["violated_at_step"]
                ),
                (&json!(expected.0), &json!(expected.1), &json!(expected.2)),
                "{engine}: {value}"
            );
        } else {
            for key in ["violation_kind", "invariant", "violated_at_step", "trace"] {
                assert_eq!(value[key], Value::Null, "{engine} {key}: {value}");
            }
        }
    }
}

macro_rules! engines {
    ($module:ident, $source:ident, $bound:expr) => {
        mod $module {
            use super::*;
            #[test]
            fn induction() {
                cell($source, $bound, "induction");
            }
            #[test]
            fn bmc() {
                cell($source, $bound, "bmc");
            }
            #[test]
            fn explicit() {
                cell($source, $bound, "explicit");
            }
        }
    };
}

engines!(g02_scalar_init, G02, Some(("_bounds_v", 0)));
engines!(g04_scalar_later, G04, Some(("_bounds_v", 1)));
engines!(g01_set_init, G01, Some(("_bounds_s", 0)));
engines!(set_later, SET_LATER, Some(("_bounds_s", 1)));
engines!(relation_init, REL_INIT, Some(("_bounds_r", 0)));
engines!(relation_later, REL_LATER, Some(("_bounds_r", 1)));
engines!(g03_scalar_valid, G03, None);
engines!(collection_valid, COLLECTION_VALID, None);
engines!(free_init, FREE_INIT, Some(("_bounds_v", 0)));
engines!(sibling_unselected, SIBLING, None);

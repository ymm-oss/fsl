// SPDX-License-Identifier: Apache-2.0

//! #1613: whole-map refusal follows abstract source scope, not residual spelling.
//! The same-name case is a detector; binder/template/element cases are preservation.
//! Production and `issue_1613_indexed_substitution.rs` are intentionally untouched.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Literal singleton witness reconstructed from the retained sibling observation.
// Abstract m denotes implementation n, never implementation m. Only m changes.
const IMPLEMENTATION: &str = "// SPDX-License-Identifier: Apache-2.0
spec Impl {
  type K = 0..0
  state { m: Map<K, Int>, n: Map<K, Int>, z: Map<K, Int> }
  init { forall k: K { m[k] = 0 } forall k: K { n[k] = 0 } forall k: K { z[k] = 0 } }
  fair action go(k: K) { requires m[k] == 0 m[k] = 1 }
}
";

const WHOLE: &str = "// SPDX-License-Identifier: Apache-2.0
spec Abs {
  type K = 0..0
  state { m: Map<K, Int>, z: Map<K, Int> }
  init { forall k: K { m[k] = 0 } forall k: K { z[k] = 0 } }
  fair action step(k: K) { requires m[k] == 0 m[k] = 1 }
  leadsTo L { m == z ~> m != z }
}
";

const ELEMENTS: &str = "// SPDX-License-Identifier: Apache-2.0
spec Abs {
  type K = 0..0
  state { m: Map<K, Int>, z: Map<K, Int> }
  init { forall k: K { m[k] = 0 } forall k: K { z[k] = 0 } }
  fair action step(k: K) { requires m[k] == 0 m[k] = 1 }
  leadsTo L { forall k: K { m[k] == z[k] ~> m[k] != z[k] } }
}
";

const MAPPING: &str = "// SPDX-License-Identifier: Apache-2.0
refinement R {
  impl Impl
  abs Abs
  map m[i: K] = n[i]
  map z[j: K] = z[j]
  action go(k) -> stutter
  preserve progress { respond L by go }
}
";

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, implementation: &str, abstraction: &str, mapping: &str) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.codex/tmp/issue-1613-whole-scope")
            .join(format!("{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create own evidence directory");
        for (file, source) in [
            ("impl.fsl", implementation),
            ("abs.fsl", abstraction),
            ("map.fsl", mapping),
        ] {
            std::fs::write(dir.join(file), source).unwrap();
        }
        Self(dir)
    }

    fn run(&self, label: &str, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .env_remove("FSLC_CACHE_VERIFY")
            .env("FSLC_CACHE_DIR", self.0.join("cache"))
            .output()
            .expect("execute native fslc subprocess");
        // Preserve raw observations even when the intended assertion goes RED.
        std::fs::write(self.0.join(format!("{label}.stdout.json")), &output.stdout).unwrap();
        std::fs::write(self.0.join(format!("{label}.stderr")), &output.stderr).unwrap();
        println!(
            "OBS fixture={} binary={} args={args:?} exit={:?}\nstdout={}\nstderr={}",
            self.0.display(),
            env!("CARGO_BIN_EXE_fslc"),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn twice(&self, label: &str, args: &[&str]) -> (i32, Value) {
        let first = self.run(&format!("{label}-1"), args);
        let second = self.run(&format!("{label}-2"), args);
        assert_eq!(first.status.code(), second.status.code(), "{label}");
        assert_eq!(first.stderr, second.stderr, "{label}: complete stderr");
        assert!(first.stderr.is_empty(), "{first:?}");
        let a: Value = serde_json::from_slice(&first.stdout).expect("full CLI JSON");
        let b: Value = serde_json::from_slice(&second.stdout).expect("full CLI JSON");
        // No normalization, selected-field comparison, or excluded cache fields.
        assert_eq!(a, b, "{label}: complete JSON");
        assert_eq!(first.stdout, second.stdout, "{label}: raw stdout");
        assert_eq!(a["fsl"], "1.0", "{a:#}");
        (first.status.code().expect("native exit code"), a)
    }

    fn check_specs(&self) {
        for file in ["impl.fsl", "abs.fsl"] {
            let (exit, value) = self.twice(file, &["check", file]);
            assert_eq!(exit, 0, "{value:#}");
            assert_eq!(value["result"], "ok", "{value:#}");
        }
    }

    fn refine(&self) -> (i32, Value) {
        self.twice(
            "refine",
            &["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"],
        )
    }
}

fn no_trace(value: &Value) {
    for key in ["impl_trace", "trace", "trace_type", "abs_trace"] {
        assert!(value.get(key).is_none(), "unexpected {key}: {value:#}");
    }
}

fn located_whole_map_error((exit, value): (i32, Value)) {
    assert_eq!(exit, 2, "expected located refusal, never proof: {value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "type", "{value:#}");
    assert_eq!(value["impl"], "Impl", "{value:#}");
    assert_eq!(value["abs"], "Abs", "{value:#}");
    assert_eq!(value["loc"], json!({"line": 8, "column": 23}), "{value:#}");
    assert_eq!(
        value["span"],
        json!({
            "start": {"offset": 166, "line": 8, "column": 23},
            "end": {"offset": 173, "line": 8, "column": 30}
        }),
        "{value:#}"
    );
    assert_eq!(
        value["message"], "cannot pull whole-map read 'm' through an indexed state map",
        "{value:#}"
    );
    assert!(value.get("progress").is_none(), "no proof: {value:#}");
    assert!(value.get("checked_to_depth").is_none(), "{value:#}");
    no_trace(&value);
}

#[test]
fn same_name_nonidentity_whole_map_is_a_located_error_twice() {
    let fixture = Fixture::new("whole-same", IMPLEMENTATION, WHOLE, MAPPING);
    fixture.check_specs();
    located_whole_map_error(fixture.refine());
}

#[test]
fn renaming_only_unrelated_implementation_m_preserves_located_error_twice() {
    // Rename only implementation m and its uses in this literal.
    // Abstract source and mapping are byte-identical to the same-name detector.
    let renamed = IMPLEMENTATION.replace("m:", "x:").replace("m[", "x[");
    assert_eq!(
        renamed.replace("x:", "m:").replace("x[", "m["),
        IMPLEMENTATION
    );
    let fixture = Fixture::new("whole-renamed", &renamed, WHOLE, MAPPING);
    fixture.check_specs();
    located_whole_map_error(fixture.refine());
}

fn progress_lost((exit, value): (i32, Value)) -> Value {
    assert_eq!(exit, 1, "{value:#}");
    assert_eq!(value["result"], "refinement_failed", "{value:#}");
    assert_eq!(value["kind"], "progress_lost", "{value:#}");
    assert_eq!(value["violation_kind"], "leadsTo", "{value:#}");
    assert_eq!(value["invariant"], "L", "{value:#}");
    assert_eq!(value["impl"], "Impl", "{value:#}");
    assert_eq!(value["abs"], "Abs", "{value:#}");
    assert_eq!(value["bindings"], json!({"k": 0}), "{value:#}");
    assert_eq!(value["loc"], json!({"line": 7, "column": 3}), "{value:#}");
    assert_eq!(
        value["progress"],
        json!({"leadsTo": "L", "actions": ["go"]})
    );
    assert_eq!(value["trace_type"], "refinement", "{value:#}");
    assert_eq!(
        value["faithfulness_class"], "liveness_not_refined",
        "{value:#}"
    );
    assert_eq!(value["pending_since"], 0, "{value:#}");
    assert_eq!(value["stutter"], true, "{value:#}");
    assert_eq!(
        value["progress_failure"], "deadlock_or_stall_blocks_progress",
        "{value:#}"
    );
    let trace = value["impl_trace"]
        .as_array()
        .expect("real implementation trace");
    assert_eq!(trace.len(), 2, "{value:#}");
    assert_eq!(
        trace[0]["state"],
        json!({"m": {"0": 0}, "n": {"0": 0}, "z": {"0": 0}})
    );
    assert_eq!(
        trace[1]["state"],
        json!({"m": {"0": 1}, "n": {"0": 0}, "z": {"0": 0}})
    );
    assert_eq!(
        trace[1]["action"],
        json!({"name": "go", "params": {"k": 0}, "loc": {"line": 6, "column": 3}}),
        "{value:#}"
    );
    assert_eq!(trace[1]["changes"], json!({"m[0]": {"from": 0, "to": 1}}));
    value
}

#[test]
fn singleton_element_reads_admit_and_lose_progress_twice() {
    let fixture = Fixture::new("elements", IMPLEMENTATION, ELEMENTS, MAPPING);
    fixture.check_specs();
    progress_lost(fixture.refine());
}

fn reflexive_binder(name: &str, property: &str) {
    let abstraction = WHOLE.replace("m == z ~> m != z", property);
    let fixture = Fixture::new(name, IMPLEMENTATION, &abstraction, MAPPING);
    fixture.check_specs();
    let (exit, value) = fixture.refine();
    assert_eq!(exit, 0, "{value:#}");
    assert_eq!(value["result"], "refines", "{value:#}");
    assert_eq!(value["checked_to_depth"], 4, "{value:#}");
    assert_eq!(value["action_map"], json!({"go": "stutter"}), "{value:#}");
    assert_eq!(
        value["progress"],
        json!({"L": {"actions": ["go"], "checked_to_depth": 4}})
    );
    no_trace(&value);
}

#[test]
fn scalar_source_binder_named_m_shadows_abstract_map_twice() {
    // m is a scalar K here. Its map is unused; no free implementation capture.
    reflexive_binder("binder", "forall m: K { m == m ~> m == m }");
}

#[test]
fn nested_source_binder_named_m_remains_lexically_bound_twice() {
    reflexive_binder(
        "binder-nested",
        "forall k: K { (forall m: K { m == k }) ~> (forall m: K { m == k }) }",
    );
}

#[test]
fn implementation_whole_map_reads_inside_indexed_int_template_remain_legal_twice() {
    // In every reachable impl state n == z == {0: 0}; the selected Int is n[i].
    // The abstract property contains only element reads, no whole-map read.
    // Conditional expressions use then/else, not statement-style brace arms.
    let mapping = MAPPING.replace("= n[i]", "= if n == z then n[i] else 1");
    let fixture = Fixture::new("template", IMPLEMENTATION, ELEMENTS, &mapping);
    fixture.check_specs();
    let template = progress_lost(fixture.refine());
    let element = Fixture::new(
        "template-element-control",
        IMPLEMENTATION,
        ELEMENTS,
        MAPPING,
    );
    element.check_specs();
    // Equal selected mapping values imply equal complete refinement observations.
    // This is a full JSON agreement comparison, not a hand-picked trace projection.
    assert_eq!(template, progress_lost(element.refine()));
}

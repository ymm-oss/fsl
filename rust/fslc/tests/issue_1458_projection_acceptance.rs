// SPDX-License-Identifier: Apache-2.0

//! #1458: accepted projection families, rejecting Set boundaries and Kernel types.
//! All seven tests detect reverting only endpoint-specific projection inference.

use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

const COUNT: &str = r"spec P {
  enum Color { Red, Blue }
  type N = 0..2
  type M = 0..4
  state { s: relation Color -> Color }
  init { s = Set {} }
  action links(a: Color, b: Color) { s = s.add(a, b) }
  invariant P { count(x in domain(s)) <= 1 }
}
";

const SET_EQUALITY: &str = r"spec P {
  enum Color { Red, Blue }
  type N = 0..2
  type M = 0..4
  state { s: relation Color -> Color }
  init { s = Set {} }
  action links(a: Color, b: Color) { s = s.add(a, b) }
  invariant P { domain(s) == Set {} or domain(s) == Set { Red } }
}
";

const COLOR_ASSIGNMENT: &str = r"spec P {
  enum Color { Red, Blue }
  state { s: relation Color -> Color, xs: Set<Color> }
  init { s = Set {}
         xs = Set {} }
  action links(a: Color, b: Color) { s = s.add(a, b) }
  action snap() { xs = domain(s) }
  invariant P { not xs.contains(Red) }
}
";

const NUMERIC_ASSIGNMENT: &str = r"spec P {
  enum Color { Red, Blue }
  type N = 0..2
  state { s: relation Color -> Color, xs: Set<N> }
  init { s = Set {}
         xs = Set {} }
  action links(a: Color, b: Color) { s = s.add(a, b) }
  action snap() { xs = domain(s) }
  invariant P { not xs.contains(0) }
}
";

const IMPLEMENTATION: &str = r"spec Impl {
  enum Color { Red, Blue }
  state { s: relation Color -> Color }
  init { s = Set {} }
  action links(a: Color, b: Color) { s = s.add(a, b) }
}
";

const COLOR_ABSTRACTION: &str = r"spec AbsC {
  enum Color { Red, Blue }
  state { xs: Set<Color> }
  init { xs = Set {} }
  action mark(a: Color) { xs = xs.add(a) }
  action keep() { }
}
";

const NUMERIC_ABSTRACTION: &str = r"spec AbsN {
  type N = 0..2
  state { xs: Set<N> }
  init { xs = Set {} }
  action mark(a: N) { xs = xs.add(a) }
  action keep() { }
}
";

const COLOR_MAP: &str = r"refinement R {
  impl Impl
  abs AbsC
  map xs = domain(s)
  action links(a, b) -> mark(a)
}
";

const NUMERIC_MAP: &str = r"refinement R {
  impl Impl
  abs AbsN
  map xs = domain(s)
  action links(a, b) -> keep()
}
";

fn scratch(label: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1458_projection_acceptance")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned projection fixtures");
    directory
}

fn run(directory: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run native projection command");
    assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn model_commands(label: &str, source: &str) -> [(Value, i32); 3] {
    let directory = scratch(label);
    std::fs::write(directory.join("projection.fsl"), source).expect("write inline projection");
    // Collect both engines even when checking is wrong, so a check assertion
    // cannot hide either engine during revert calibration.
    [
        run(&directory, &["check", "projection.fsl"]),
        run(
            &directory,
            &[
                "verify",
                "projection.fsl",
                "--engine",
                "bmc",
                "--depth",
                "3",
                "--no-cache",
            ],
        ),
        run(
            &directory,
            &[
                "verify",
                "projection.fsl",
                "--engine",
                "explicit",
                "--depth",
                "3",
                "--no-cache",
            ],
        ),
    ]
}

fn assert_violation(label: &str, source: &str, step: u64) {
    let outputs = model_commands(label, source);
    for (index, (output, status)) in outputs.iter().enumerate() {
        let expected = if index == 0 {
            (0, "ok")
        } else {
            (1, "violated")
        };
        assert_eq!(
            (*status, output["result"].as_str(), output["kind"].as_str()),
            (expected.0, Some(expected.1), None),
            "{label}: all commands: {outputs:#?}"
        );
        if index != 0 {
            assert_eq!(output["violation_kind"], "invariant", "{output:#}");
            assert_eq!(output["invariant"], "P", "{output:#}");
            assert_eq!(output["violated_at_step"], step, "{output:#}");
            assert_eq!(output["loc"], json!({"line": 8, "column": 3}), "{output:#}");
        }
    }
}

fn refine(label: &str, abstraction: &str, mapping: &str) -> (Value, i32) {
    let directory = scratch(label);
    for (name, source) in [
        ("impl.fsl", IMPLEMENTATION),
        ("abs.fsl", abstraction),
        ("map.fsl", mapping),
    ] {
        std::fs::write(directory.join(name), source).expect("write inline refinement fixture");
    }
    run(
        &directory,
        &["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "3"],
    )
}

#[test]
fn count_over_enum_domain_reports_a_real_violation() {
    assert_violation("count", COUNT, 2);
}

#[test]
fn enum_domain_set_literal_equality_reports_a_real_violation() {
    assert_violation("set-equality", SET_EQUALITY, 1);
}

#[test]
fn enum_domain_assignment_to_enum_set_reports_a_real_violation() {
    assert_violation("color-assignment", COLOR_ASSIGNMENT, 2);
}

#[test]
fn enum_domain_assignment_to_numeric_set_is_a_located_type_error() {
    let outputs = model_commands("numeric-assignment", NUMERIC_ASSIGNMENT);
    for (output, status) in &outputs {
        assert_eq!(
            (*status, output["result"].as_str(), output["kind"].as_str()),
            (2, Some("error"), Some("semantics")),
            "all commands: {outputs:#?}"
        );
        assert_eq!(
            output["loc"],
            json!({"line": 8, "column": 19}),
            "{output:#}"
        );
        assert!(output["message"].as_str().is_some_and(|message| {
            message.contains("expression of type Set(Named(\"Color\")) is not assignable to Set(Named(\"N\"))")
        }), "{output:#}");
    }
}

#[test]
fn enum_domain_refinement_map_to_enum_set_refines() {
    let (output, status) = refine("color-map", COLOR_ABSTRACTION, COLOR_MAP);
    assert_eq!(
        (status, output["result"].as_str()),
        (0, Some("refines")),
        "{output:#}"
    );
    assert_eq!(output["checked_to_depth"], 3, "{output:#}");
    assert_eq!(output["action_map"], json!({"links": "mark"}), "{output:#}");
    assert!(output.get("kind").is_none(), "{output:#}");
    // Successful refinement has no diagnostic origin in the native envelope.
    assert!(output.get("loc").is_none(), "{output:#}");
}

#[test]
fn enum_domain_refinement_map_to_numeric_set_is_rejected_before_simulation() {
    let (output, status) = refine("numeric-map", NUMERIC_ABSTRACTION, NUMERIC_MAP);
    assert_eq!(
        (status, output["result"].as_str(), output["kind"].as_str()),
        (2, Some("error"), Some("type")),
        "{output:#}"
    );
    assert!(output["message"].as_str().is_some_and(|message| {
        message.contains("invalid map expression for 'xs': expression of type Set(Named(\"Color\")) is not assignable to Set(Named(\"N\"))")
    }), "{output:#}");
    // Existing refinement map diagnostics use the unknown-span origin, not
    // the authored map keyword. Pin that limitation without inventing a span.
    assert_eq!(output["loc"], json!({"line": 1, "column": 1}), "{output:#}");
    assert!(output.get("impl_trace").is_none(), "{output:#}");
}

#[test]
fn public_kernel_projection_items_and_binders_keep_both_endpoint_orders() {
    for (source_type, target_type) in [("N", "Color"), ("Color", "N")] {
        let directory = scratch(&format!("kernel-{source_type}-{target_type}"));
        // Self-comparison is well typed even with the old Int inference, so
        // reverting the hunk fails on annotations, not model construction.
        let source = format!(
            "spec ProjectionKernel {{\n  enum Color {{ Red, Blue }}\n  type N = 0..2\n  state {{ r: relation {source_type} -> {target_type} }}\n  init {{ r = Set {{}} }}\n  action stay() {{}}\n  invariant D {{ forall x in domain(r): x == x }}\n  invariant R {{ forall y in range(r): y == y }}\n}}\n"
        );
        let kernel = parse_kernel_source(&source, &FsResolver::new(&directory))
            .expect("parse Kernel fixture");
        let model = build_model(kernel.clone()).expect("build well-typed Kernel fixture");
        let contract =
            fsl_core::public_kernel_contract(&kernel, &model, "projection.fsl", "kernel")
                .expect("export typed Kernel");
        for (index, operation, endpoint) in [
            (0, "rel_domain", source_type),
            (1, "rel_range", target_type),
        ] {
            let expression = &contract["properties"]["invariants"][index]["expression"];
            let binder = &expression["binder"];
            let collection = &binder["collection"];
            let item = json!({"kind": "named", "name": endpoint});
            assert_eq!(collection["kind"], operation, "{contract:#}");
            assert_eq!(
                collection["type"],
                json!({"kind": "set", "item": item}),
                "{contract:#}"
            );
            assert_eq!(binder["type"], item, "{contract:#}");
        }
    }
}

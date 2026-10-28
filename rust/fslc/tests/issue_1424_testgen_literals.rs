// SPDX-License-Identifier: Apache-2.0

//! #1424: target literals, precision rejection, and context-safe source headers.
//! CLI tests own delivery/errors. Adapted real Public Kernel/scenario/walk JSON
//! covers strings and isolates literal positions in scenario/conformance vectors.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[cfg(unix)]
const TARGETS: [&str; 6] = ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"];
const SOURCE: &str = "spec Literals {\n  type N = 0..1\n  state { x: Int }\n  init { x = 0 }\n  action set(v: N) { x = v }\n  reachable One { x == 1 }\n}\n";
static NEXT: AtomicU64 = AtomicU64::new(0);

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
}

fn scratch(label: &str) -> PathBuf {
    let dir = root().join(format!(
        "rust/target/issue-1424-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("worktree-local scratch directory");
    dir
}

fn cli(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("native fslc")
}

fn generated(spec: &str, target: &str, cwd: &Path) -> String {
    // Match testgen_contract's output-parent depth, so pytest's SPEC_PATH is
    // ../../../specs/cart_v1.fsl rather than an ambient absolute checkout path.
    let output_path = scratch("output").join("generated.out");
    let output = cli(
        &[
            "testgen",
            spec,
            "--depth",
            "3",
            "--target",
            target,
            "-o",
            output_path.to_str().expect("UTF-8 output path"),
        ],
        cwd,
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("generated envelope");
    assert_eq!(envelope["result"], "generated");
    std::fs::read_to_string(output_path).expect("generated source file")
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Remove precisely one live helper, not its comments or any other output.
/// The block terminator is at column zero; nested Kotlin blocks are indented.
fn split_helper(text: &str) -> (&str, String) {
    assert_eq!(text.matches("fun assertPartial(").count(), 1);
    let start = text.find("fun assertPartial(").expect("Kotlin helper");
    let end = start + text[start..].find("\n}\n").expect("helper closing brace") + 3;
    (
        &text[start..end],
        format!("{}{}", &text[..start], &text[end..]),
    )
}

#[test]
fn preservation_ordinary_cli_outputs_match_base_bytes() {
    // Captured on dd376efd, with a fixed output-parent depth. Compare every
    // artifact byte: no fields are removed or normalized for these targets.
    let expected = [
        (
            "pytest",
            "89454a73cbcc05d24fdbde88e30e97299c5eac7ac7098a06a62dc8d4a35076fb",
        ),
        (
            "vitest",
            "805c1659c48f50a3206c7c452a5113b5737780275c90297b4edb4c11ff997b96",
        ),
        (
            "swift",
            "69b6c6e728e6165b4130ab4635562d7931b705b8156325b62c5db163d0da9c92",
        ),
        (
            "dart",
            "a23ffda2fbb99e906c1f0afb3ef6758941c8200a119538bdef14063107b95e9d",
        ),
        (
            "phpunit",
            "c3c38c14bbae0aa96c3336c4e0bf9020720b6ca1ed69cdd27626fe5d774188cf",
        ),
    ];
    let actual = expected.map(|(target, expected)| {
        let content = generated("specs/cart_v1.fsl", target, root());
        (target, digest(&content), expected)
    });
    assert!(
        actual
            .iter()
            .all(|(_, actual, expected)| actual == expected),
        "{actual:#?}"
    );
}

#[test]
fn preservation_kotlin_changes_only_inside_assert_partial() {
    let content = generated("specs/cart_v1.fsl", "kotlin", root());
    let (helper, outside) = split_helper(&content);
    assert!(helper.contains("assertEquals"), "live helper exclusion");
    assert_eq!(
        digest(&outside),
        "702342db2d16523f12a6e3a94a37e166f0591d674b93c1c63e2bb982e2a63256",
        "all bytes outside assertPartial"
    );
}

#[test]
fn detector_kotlin_helper_compares_integral_numbers_by_value() {
    let content = generated("specs/cart_v1.fsl", "kotlin", root());
    let (helper, _) = split_helper(&content);
    assert!(
        helper.contains("is Int")
            && helper.contains("is Long")
            && helper.matches(".toLong()").count() >= 2,
        "Int 5 and Long 5 need integral-by-value comparison, not Any?.equals:\n{helper}"
    );
    assert!(
        !helper.contains(".toDouble()"),
        "must not coerce non-integral numbers"
    );
    assert!(helper.contains("assertPartial(") && helper.contains("assertEquals"));
}

/// Reuse real generated JSON, changing only the selected literal position.
/// This tests the public adapter/emitter, not acceptance of these values as FSL.
fn adapted_input(value: Value, position: &str) -> fsl_tools::TestgenInput {
    let dir = scratch("api");
    std::fs::write(dir.join("literal.fsl"), SOURCE).expect("write real model");
    let scenarios_output = cli(&["scenarios", "literal.fsl", "--depth", "3"], &dir);
    assert_eq!(
        scenarios_output.status.code(),
        Some(0),
        "{scenarios_output:?}"
    );
    let mut scenarios: Value = serde_json::from_slice(&scenarios_output.stdout).expect("scenarios");
    let kernel = fsl_core::parse_kernel_source(SOURCE, &fsl_core::FsResolver::new("."))
        .expect("parse real model");
    let model = fsl_core::build_model(kernel.clone()).expect("build real model");
    let fslc_rust::TestgenWalk::Clean(mut walk) =
        fslc_rust::testgen_trace_vectors(&model).expect("real walk")
    else {
        panic!("clean literal model")
    };
    let scenario = scenarios["scenarios"]
        .as_array_mut()
        .expect("scenario array")
        .first_mut()
        .expect("real scenario");
    assert!(!scenario["steps"].as_array().expect("steps").is_empty());
    match position {
        "walk.initial" => walk["initial"]["x"] = value,
        "walk.params" => walk["steps"][0]["params"]["v"] = value,
        "walk.expected" => walk["steps"][0]["expected"]["x"] = value,
        "scenario.params" => scenario["steps"][0]["params"]["v"] = value,
        "scenario.expected" => scenario["expected_states"][0]["x"] = value,
        _ => panic!("unknown literal position"),
    }
    let contract = fsl_core::public_kernel_contract(&kernel, &model, "literal.fsl", "spec")
        .expect("Public Kernel");
    let path = fsl_tools::TestgenPathContext::without_output(
        Path::new("literal.fsl"),
        PathBuf::from("literal.fsl"),
    )
    .expect("path context");
    fsl_tools::public_kernel_testgen_input(&contract, &path, &scenarios, &walk)
        .expect("adapt real JSON")
}

#[test]
fn detector_kotlin_cli_emits_wide_and_minimum_literals() {
    let dir = scratch("long-cli");
    let mut failures = Vec::new();
    // Spell MIN as arithmetic so the positive magnitude need not overflow i64
    // while parsing the unary minus operand.
    for (expression, expected) in [
        ("2147483648", "2147483648L"),
        ("-9223372036854775807 - 1", "Long.MIN_VALUE"),
    ] {
        std::fs::write(
            dir.join("literal.fsl"),
            SOURCE.replace("x = 0", &format!("x = {expression}")),
        )
        .expect("write integer model");
        let content = generated("literal.fsl", "kotlin", &dir);
        if !content.contains(&format!("\"x\" to {expected}")) {
            failures.push(format!(
                "CLI initial {expression} must render as {expected}"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn detector_kotlin_wide_and_minimum_literals_cover_all_value_paths() {
    let mut failures = Vec::new();
    for position in [
        "walk.initial",
        "walk.params",
        "walk.expected",
        "scenario.params",
        "scenario.expected",
    ] {
        for (value, spelling) in [
            (2_147_483_648_i64, "2147483648L"),
            (-2_147_483_649, "-2147483649L"),
            (i64::MIN, "Long.MIN_VALUE"),
        ] {
            let content =
                fsl_tools::generate_testgen(&adapted_input(json!(value), position), "kotlin")
                    .expect("Kotlin generation");
            let key = if position.ends_with("params") {
                "v"
            } else {
                "x"
            };
            if !content.contains(&format!("\"{key}\" to {spelling}")) {
                failures.push(format!("{position}: {value} must be {spelling}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn preservation_kotlin_int_range_literals_have_no_long_suffix() {
    for value in [i64::from(i32::MIN), -1, 0, 5, i64::from(i32::MAX)] {
        let content =
            fsl_tools::generate_testgen(&adapted_input(json!(value), "walk.initial"), "kotlin")
                .expect("Kotlin");
        assert!(
            content.contains(&format!("\"x\" to {value})")),
            "Int-range {value} stays unsuffixed"
        );
        assert!(!content.contains(&format!("{value}L")));
    }
}

#[test]
fn detector_vitest_cli_rejects_lossy_integer_with_located_exit_two() {
    let dir = scratch("unsafe-js");
    let source = SOURCE.replace("x = 0", "x = 9007199254740993");
    std::fs::write(dir.join("literal.fsl"), source).expect("write wide integer model");
    let output = cli(
        &[
            "testgen",
            "literal.fsl",
            "--target",
            "vitest",
            "--depth",
            "1",
        ],
        &dir,
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "unsafe JS integer must fail closed, not emit generated code"
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("error envelope");
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["kind"], "semantics");
    let message = envelope["message"].as_str().expect("error message");
    assert!(message.contains("9007199254740993"), "{message}");
    assert!(
        message.contains("initial") && message.contains('x'),
        "literal position must be located: {message}"
    );
    assert!(envelope.get("content").is_none(), "no lossy artifact");
}

#[test]
fn detector_vitest_rejects_unsafe_values_in_each_public_vector_position() {
    let mut failures = Vec::new();
    for position in [
        "walk.initial",
        "walk.params",
        "walk.expected",
        "scenario.params",
        "scenario.expected",
    ] {
        for value in [9_007_199_254_740_993_i64, -9_007_199_254_740_993] {
            match fsl_tools::generate_testgen(&adapted_input(json!(value), position), "vitest") {
                Ok(_) => failures.push(format!("{position}: emitted lossy {value}")),
                Err(message) => assert!(message.contains(&value.to_string()), "{message}"),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn preservation_vitest_safe_integer_boundaries_are_exact() {
    for value in [-9_007_199_254_740_991_i64, 0, 9_007_199_254_740_991] {
        let content =
            fsl_tools::generate_testgen(&adapted_input(json!(value), "walk.initial"), "vitest")
                .expect("safe JS integer");
        assert!(
            content.contains(&format!("\"x\": {value}")),
            "{value} remains exact"
        );
    }
}

#[test]
fn detector_pytest_escapes_every_ascii_control_in_each_value_path() {
    let value = format!(
        "before{}after\\'",
        (0_u8..=31)
            .map(char::from)
            .chain([char::from(127)])
            .collect::<String>()
    );
    let expected = (0_u8..=31)
        .chain([127])
        .map(|byte| match byte {
            b'\n' => "\\n".to_owned(),
            b'\r' => "\\r".to_owned(),
            b'\t' => "\\t".to_owned(),
            other => format!("\\x{other:02x}"),
        })
        .collect::<String>();
    let mut failures = Vec::new();
    for position in ["scenario.params", "scenario.expected"] {
        let content = fsl_tools::generate_testgen(&adapted_input(json!(value), position), "pytest")
            .expect("pytest");
        if !content.contains(&format!("before{expected}after\\\\\\'")) {
            failures.push(format!(
                "{position}: control string must use Python escapes"
            ));
        }
        assert!(
            !content.contains("before\\x00after"),
            "do not truncate the string"
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Shared accepted S1 spelling: dangerous header bytes become lowercase \xNN.
/// This is presentation escaping, not a target-language string literal encoder.
#[cfg(unix)]
fn escaped_source(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_control() || matches!(ch, '\\' | '\'' | '"' | '*' | '/' | '?' | '>') {
                format!("\\x{:02x}", u32::from(ch))
            } else {
                ch.to_string()
            }
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn detector_source_headers_escape_context_switches_identically_in_every_target() {
    let dir = scratch("source-names");
    let mut failures = Vec::new();
    for name in [
        "triple\"\"\".fsl",
        "triple'''.fsl",
        "close?>.fsl",
        "star*.fsl",
        "back\\slash.fsl",
        "line\nfeed.fsl",
        "control\r\u{1}.fsl",
        "非ASCII.fsl",
    ] {
        std::fs::write(dir.join(name), SOURCE).expect("legal Unix source basename");
        let expected = escaped_source(name);
        for target in TARGETS {
            let content = generated(name, target, &dir);
            let source_lines = content
                .lines()
                .filter_map(|line| line.split_once("Source: ").map(|(_, name)| name))
                .collect::<Vec<_>>();
            if source_lines != [expected.as_str()] {
                failures.push(format!(
                    "{target}/{name:?}: expected {expected:?}, got {source_lines:?}"
                ));
            }
        }
    }
    // '/' is not legal in a basename. A real path containing */ must not leak
    // directory components into __SOURCE__ (all targets use the basename).
    let star = dir.join("star*");
    std::fs::create_dir_all(&star).expect("star directory");
    std::fs::write(star.join("ordinary.fsl"), SOURCE).expect("spec under */ path");
    for target in TARGETS {
        let content = generated("star*/ordinary.fsl", target, &dir);
        assert!(
            content
                .lines()
                .any(|line| line.ends_with("Source: ordinary.fsl")),
            "{target}: basename-only header"
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

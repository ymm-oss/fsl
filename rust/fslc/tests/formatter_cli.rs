// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::{collections::BTreeMap, ffi::OsStr};

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run fslc")
}

fn run_stdin(args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run fslc");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("write source");
    child.wait_with_output().expect("wait for fslc")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

#[test]
fn fmt_stdout_stdin_check_and_error_contracts_are_non_destructive() {
    let root = root();
    let fixture =
        root.join("rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl");
    let original = fs::read_to_string(&fixture).expect("fixture");

    let formatted = run(&["fmt", fixture.to_str().expect("path"), "--edition", "next"]);
    assert!(formatted.status.success());
    let formatted_source = String::from_utf8(formatted.stdout).expect("formatted UTF-8");
    assert!(formatted_source.starts_with("domain DomainExpressionCharacterization"));
    assert!(formatted_source.contains("enum OrderStatus"));
    assert_eq!(fs::read_to_string(&fixture).expect("unchanged"), original);

    let stdin = run_stdin(&["fmt", "-", "--edition", "next"], &original);
    assert!(stdin.status.success());
    assert_eq!(stdin.stdout, formatted_source.as_bytes());

    let directory = root.join(format!("rust/target/formatter-cli-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("temp directory");
    let dirty = directory.join("dirty.fsl");
    fs::write(
        &dirty,
        "spec Tiny{state{x:Bool}init{x=false}invariant P{x==false}}",
    )
    .expect("dirty fixture");
    let check = run(&["fmt", dirty.to_str().expect("path"), "--check"]);
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(json(&check)["result"], "format_check");
    assert_eq!(json(&check)["changed"], true);
    assert_eq!(
        fs::read_to_string(&dirty).expect("check is non-mutating"),
        "spec Tiny{state{x:Bool}init{x=false}invariant P{x==false}}"
    );

    let malformed = directory.join("malformed.fsl");
    fs::write(&malformed, "spec Broken { invariant P { x ` y } }").expect("malformed");
    let error = run(&["fmt", malformed.to_str().expect("path"), "--check"]);
    assert_eq!(error.status.code(), Some(2));
    assert_eq!(json(&error)["result"], "error");
    assert!(json(&error)["loc"]["line"].as_u64().is_some());
    assert_eq!(
        fs::read_to_string(&malformed).expect("error is non-mutating"),
        "spec Broken { invariant P { x ` y } }"
    );
}

#[test]
fn registry_dialects_format_idempotently_or_refuse_the_opaque_agent_boundary() {
    for path in [
        "specs/cart_v1.fsl",
        "specs/seat_refines.fsl",
        "specs/bank_system.fsl",
        "examples/e2e/1_business.fsl",
        "examples/consulting/governance_controls.fsl",
        "examples/e2e/2_requirements.fsl",
        "rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl",
        "examples/db/safe_add_nullable_column.fsl",
        "examples/ai/refund_agent_tool_safety.fsl",
    ] {
        let first = run(&["fmt", path]);
        assert!(
            first.status.success(),
            "{path}: {}",
            String::from_utf8_lossy(&first.stdout)
        );
        let source = String::from_utf8(first.stdout).expect("formatted UTF-8");
        let second = run_stdin(&["fmt", "-"], &source);
        assert!(second.status.success(), "{path} second format");
        assert_eq!(second.stdout, source.as_bytes(), "{path} is not idempotent");
    }

    let agent = run(&["fmt", "examples/ai/recursive_support_agent.fsl"]);
    assert_eq!(agent.status.code(), Some(2));
    assert_eq!(json(&agent)["code"], "FSL-FMT-UNSAFE");
}

#[test]
fn fmt_help_and_multiple_path_contract_are_fixed() {
    let help = run(&["fmt", "--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help UTF-8");
    assert!(help.contains("--check"));
    assert!(help.contains("--edition {current,next}"));

    let multiple = run(&["fmt", "specs/cart_v1.fsl", "specs/payment.fsl"]);
    assert_eq!(multiple.status.code(), Some(2));
    assert_eq!(json(&multiple)["kind"], "usage");

    let check = run(&["fmt", "specs/cart_v1.fsl", "specs/payment.fsl", "--check"]);
    assert!(matches!(check.status.code(), Some(0 | 1)));
    assert_eq!(json(&check)["files"].as_array().map(Vec::len), Some(2));
}

#[test]
fn business_ids_keep_canonical_hyphens_without_compacting_subtraction() {
    let source = r#"business Repro {
  actor Owner
  entity Case
  process Case {
    stages Open, Closed
    initial Open
    transition close Open -> Closed by Owner
  }
  control CTRL-REPRO-001 "A case must be reviewed before closure."
    owner Owner
    severity high
    applies_to Case
  policy POL-REPRO-001 "A case must be reviewed before closure."
    satisfies CTRL-REPRO-001
    every Case reaching Closed must have passed through Open
  goal GOAL-REPRO-001 "A reviewed case can close."
    satisfies CTRL-REPRO-001
    some Case can reach Closed
  policy POL-ARITH-001 "Subtraction remains an expression."
    invariant { 2-1 == 1 }
}
verify { instances Case = 3 }
"#;
    let formatted = run_stdin(&["fmt", "-", "--edition", "next"], source);
    assert!(
        formatted.status.success(),
        "{}",
        String::from_utf8_lossy(&formatted.stdout)
    );
    let formatted = String::from_utf8(formatted.stdout).expect("formatted UTF-8");
    assert!(formatted.contains("control CTRL-REPRO-001"));
    assert!(formatted.contains("policy POL-REPRO-001"));
    assert!(formatted.contains("satisfies CTRL-REPRO-001"));
    assert!(formatted.contains("goal GOAL-REPRO-001"));
    assert!(formatted.contains("policy POL-ARITH-001"));
    assert!(formatted.contains("2 - 1 == 1"));
}

fn fsl_files(directory: &Path, output: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("scan corpus") {
        let path = entry.expect("corpus entry").path();
        if path.is_dir() {
            fsl_files(&path, output);
        } else if path.extension() == Some(OsStr::new("fsl")) {
            output.push(path);
        }
    }
}

fn comments(source: &str) -> Vec<(String, bool)> {
    let document = fsl_syntax::lossless_document(source);
    let mut token_end_line = None;
    let mut result = Vec::new();
    for node in document.nodes() {
        match node.kind {
            fsl_syntax::LosslessKind::Token(_) => token_end_line = Some(node.span.end.line),
            fsl_syntax::LosslessKind::LineComment => result.push((
                node.text.clone(),
                token_end_line == Some(node.span.start.line),
            )),
            fsl_syntax::LosslessKind::Whitespace | fsl_syntax::LosslessKind::Error => {}
        }
    }
    result
}

fn without_spans(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("span");
            for value in object.values_mut() {
                without_spans(value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(without_spans),
        _ => {}
    }
}

fn checked_source(
    source: &str,
    resolver: &fsl_core::FsResolver,
    path: &Path,
) -> Result<(fsl_core::KernelSpec, fsl_core::KernelModel), String> {
    fsl_core::parse_kernel_source_with_file(source, resolver, path.to_string_lossy())
        .map_err(|error| error.to_string())
        .and_then(|kernel| {
            fsl_core::build_model(kernel.clone())
                .map(|model| (kernel, model))
                .map_err(|error| error.to_string())
        })
}

fn assert_checked_round_trip(path: &Path, dialect: &str, source: &str, formatted: &str) -> bool {
    let resolver = fsl_core::FsResolver::new(path.parent().expect("corpus parent"));
    let (before, after) = match (
        checked_source(source, &resolver, path),
        checked_source(formatted, &resolver, path),
    ) {
        (Ok(before), Ok(after)) => (before, after),
        (Err(_), Err(_)) => return false,
        (before, after) => panic!(
            "{} changed check outcome: before={before:?}, after={after:?}",
            path.display()
        ),
    };
    let ((before_kernel, before_model), (after_kernel, after_model)) = (before, after);
    let mut before = fsl_core::public_kernel_contract(
        &before_kernel,
        &before_model,
        &path.to_string_lossy(),
        dialect,
    )
    .expect("original public Kernel");
    let mut after = fsl_core::public_kernel_contract(
        &after_kernel,
        &after_model,
        &path.to_string_lossy(),
        dialect,
    )
    .expect("formatted public Kernel");
    without_spans(&mut before);
    without_spans(&mut after);
    assert_eq!(before, after, "{}", path.display());

    let before_verdict =
        fsl_runtime::verify_explicit(before_model, 2, 10_000).map_err(|error| error.to_string());
    let after_verdict =
        fsl_runtime::verify_explicit(after_model, 2, 10_000).map_err(|error| error.to_string());
    assert_eq!(before_verdict, after_verdict, "{}", path.display());
    true
}

#[test]
fn registered_corpus_is_idempotent_comment_lossless_and_semantically_stable() {
    let root = root();
    let mut files = Vec::new();
    fsl_files(&root.join("specs"), &mut files);
    fsl_files(&root.join("examples"), &mut files);
    files.sort();
    let mut dialects = BTreeMap::<&str, usize>::new();
    let mut compared_models = 0;
    let mut compared_verdicts = 0;
    let mut stable_check_errors = 0;

    for path in files {
        let source = fs::read_to_string(&path).expect("read corpus source");
        let Ok(dialect) = fsl_syntax::dialect_keyword(&source) else {
            continue;
        };
        *dialects.entry(dialect).or_default() += 1;
        let formatted = match fsl_syntax::format_source(&source, fsl_syntax::FormatEdition::Current)
        {
            Ok(formatted) => formatted,
            Err(fsl_syntax::FormatError::Unsafe { .. }) if dialect == "agent" => continue,
            Err(fsl_syntax::FormatError::Unsafe { message, .. })
                if message.contains("legacy domain enum") =>
            {
                continue;
            }
            Err(fsl_syntax::FormatError::Parse(_))
                if dialect == "ai_component" && source.contains("\nai_action ") =>
            {
                continue;
            }
            Err(fsl_syntax::FormatError::Parse(_))
                if fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&source)).is_err() =>
            {
                continue;
            }
            Err(fsl_syntax::FormatError::Lex(_)) if fsl_syntax::lex(&source).is_err() => continue,
            Err(error) => panic!("{}: {error}", path.display()),
        };
        let next = fsl_syntax::format_source(&source, fsl_syntax::FormatEdition::Next)
            .unwrap_or_else(|error| panic!("{} next: {error}", path.display()));
        assert_eq!(
            formatted,
            next,
            "{} edition policies diverged",
            path.display()
        );
        assert_eq!(
            comments(&source),
            comments(&formatted),
            "{}",
            path.display()
        );
        assert_eq!(
            fsl_syntax::format_source(&formatted, fsl_syntax::FormatEdition::Current)
                .expect("second format"),
            formatted,
            "{}",
            path.display()
        );

        if matches!(dialect, "refinement" | "compose" | "agent") {
            let before = fsl_syntax::lex(&source)
                .expect("original tokens")
                .into_iter()
                .map(|token| token.kind)
                .collect::<Vec<_>>();
            let after = fsl_syntax::lex(&formatted)
                .expect("formatted tokens")
                .into_iter()
                .map(|token| token.kind)
                .collect::<Vec<_>>();
            assert_eq!(before, after, "{}", path.display());
            continue;
        }

        if assert_checked_round_trip(&path, dialect, &source, &formatted) {
            compared_models += 1;
            compared_verdicts += 1;
        } else {
            stable_check_errors += 1;
        }
    }

    for dialect in fsl_syntax::DIALECT_KEYWORDS {
        assert!(dialects.contains_key(dialect), "missing {dialect} corpus");
    }
    assert!(compared_models >= 100, "too few checked corpus models");
    assert_eq!(compared_verdicts, compared_models);
    assert!(
        stable_check_errors > 0,
        "corpus must exercise check refusal"
    );
}

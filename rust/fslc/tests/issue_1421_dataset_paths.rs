// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! AI1/AI2: declared sources belong to the declaring file; explicit records
//! remain caller-relative. Preserve complete statistical/error envelopes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);
const EXAMPLE: &str = "examples/ai/support_answer_quality.fsl";
const RECORDS: &str = "examples/ai/support_eval_v3.jsonl";
const BASENAME: &str = "support_eval_v3.jsonl";
const EXTERNAL: &str = "https://example.invalid/eval.jsonl";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository")
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, directory: &Path, found: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(directory).expect("read owned fixture directory") {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("owned path").to_path_buf();
            if entry.file_type().expect("file type").is_dir() {
                found.insert(relative, None);
                visit(root, &path, found);
            } else {
                assert!(entry.file_type().expect("file type").is_file());
                found.insert(
                    relative,
                    Some(std::fs::read(path).expect("raw fixture bytes")),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}

struct Inputs {
    root: PathBuf,
    directory: PathBuf,
    unrelated: PathBuf,
    spec: PathBuf,
    positive: Vec<u8>,
    negative: Vec<u8>,
}

impl Drop for Inputs {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove owned AI fixture");
    }
}

impl Inputs {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "fslc-1421-dataset-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create fixture");
        let root = root.canonicalize().expect("physical CWD");
        let directory = root.join("project");
        let unrelated = root.join("unrelated");
        std::fs::create_dir_all(&directory).expect("declaring directory");
        std::fs::create_dir_all(&unrelated).expect("unrelated CWD");
        let positive = std::fs::read(repository().join(RECORDS)).expect("existing trusted records");
        let mut negative = Vec::new();
        for line in std::str::from_utf8(&positive)
            .expect("JSONL UTF-8")
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let mut record: Value = serde_json::from_str(line).expect("existing record");
            assert!(record["outcome"].is_boolean());
            record["outcome"] = Value::Bool(false);
            negative.extend(serde_json::to_vec(&record).expect("valid failing record"));
            negative.push(b'\n');
        }
        assert_ne!(positive, negative, "parseable decoys must be distinct");
        let spec = directory.join("quality.fsl");
        let inputs = Self {
            root,
            directory,
            unrelated,
            spec,
            positive,
            negative,
        };
        inputs.declare(Some(BASENAME));
        inputs.write(&inputs.directory.join(BASENAME), &inputs.positive);
        inputs.write(&inputs.root.join(BASENAME), &inputs.negative);
        inputs.write(&inputs.unrelated.join(BASENAME), &inputs.negative);
        inputs
    }

    fn declare(&self, source: Option<&str>) {
        let template =
            std::fs::read_to_string(repository().join(EXAMPLE)).expect("existing project");
        let lines: Vec<&str> = template
            .lines()
            .filter(|line| line.trim_start().starts_with("source \""))
            .collect();
        assert_eq!(
            lines.len(),
            1,
            "replace only the existing dataset source declaration"
        );
        let replacement = source.map_or_else(String::new, |source| {
            format!(
                "  source {}",
                serde_json::to_string(source).expect("quoted source")
            )
        });
        std::fs::write(&self.spec, template.replacen(lines[0], &replacement, 1))
            .expect("controlled declaration");
    }

    fn write(&self, path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("owned data parent");
        std::fs::write(path, bytes).expect("write controlled data");
    }
}

fn eval(
    cwd: &Path,
    spec: &Path,
    records: Option<&Path>,
    property: Option<&str>,
    dataset: Option<&str>,
) -> Output {
    let mut args = vec!["ai", "eval", spec.to_str().expect("UTF-8 spec")];
    if let Some(records) = records {
        args.extend(["--records", records.to_str().expect("UTF-8 records")]);
    }
    if let Some(property) = property {
        args.extend(["--property", property]);
    }
    if let Some(dataset) = dataset {
        args.extend(["--dataset", dataset]);
    }
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("AI CLI")
}

fn checked(output: &Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}
fn quality(cwd: &Path, spec: &Path, records: Option<&Path>) -> Output {
    eval(cwd, spec, records, Some("LooseQuality"), None)
}
fn same(output: &Output, exit: i32, expected: &Value) {
    assert_eq!(
        checked(output, exit),
        *expected,
        "complete envelope without exclusions"
    );
}

#[test]
fn ai1_declared_relative_source_uses_its_file_directory_and_never_cwd_decoys() {
    let inputs = Inputs::new();
    let before = inventory(&inputs.root);
    let actual = inputs.directory.join(BASENAME);
    let expected = checked(&quality(&inputs.root, &inputs.spec, Some(&actual)), 0);
    assert_eq!(expected["result"], "statistically_supported");
    for cwd in [&inputs.root, &inputs.unrelated] {
        let decoy = checked(&quality(cwd, &inputs.spec, Some(&cwd.join(BASENAME))), 1);
        assert_eq!(decoy["result"], "statistically_unsupported");
        assert_ne!(decoy, expected);
    }
    for cwd in [&inputs.root, &inputs.directory, &inputs.unrelated] {
        same(&quality(cwd, &inputs.spec, None), 0, &expected);
    }
    assert_eq!(inventory(&inputs.root), before);
    std::fs::remove_file(&actual).expect("remove actual declaring-directory records");
    let without_actual = inventory(&inputs.root);
    let missing = checked(&quality(&inputs.unrelated, &inputs.spec, Some(&actual)), 2);
    assert_eq!(
        missing["kind"], "parse",
        "existing records-read wrapping is preserved"
    );
    same(&quality(&inputs.unrelated, &inputs.spec, None), 2, &missing);
    assert_eq!(inventory(&inputs.root), without_actual);
    inputs.write(&actual, &inputs.positive);
    same(
        &quality(&inputs.unrelated, &inputs.spec, None),
        0,
        &expected,
    );
    assert_eq!(inventory(&inputs.root), before);
}

#[test]
fn ai1_absolute_declared_source_is_unchanged_and_has_no_missing_file_fallback() {
    let inputs = Inputs::new();
    let actual = inputs.root.join("absolute-data.jsonl");
    inputs.write(&actual, &inputs.positive);
    inputs.declare(Some(actual.to_str().expect("absolute source")));
    inputs.write(
        &inputs.unrelated.join("absolute-data.jsonl"),
        &inputs.negative,
    );
    let before = inventory(&inputs.root);
    let expected = checked(&quality(&inputs.root, &inputs.spec, Some(&actual)), 0);
    assert_eq!(expected["result"], "statistically_supported");
    let decoy = checked(
        &quality(
            &inputs.unrelated,
            &inputs.spec,
            Some(&inputs.unrelated.join("absolute-data.jsonl")),
        ),
        1,
    );
    assert_eq!(decoy["result"], "statistically_unsupported");
    for cwd in [&inputs.root, &inputs.directory, &inputs.unrelated] {
        same(&quality(cwd, &inputs.spec, None), 0, &expected);
    }
    std::fs::remove_file(&actual).expect("missing absolute source");
    let without_actual = inventory(&inputs.root);
    let missing = checked(&quality(&inputs.unrelated, &inputs.spec, Some(&actual)), 2);
    assert_eq!(missing["kind"], "parse");
    same(&quality(&inputs.unrelated, &inputs.spec, None), 2, &missing);
    assert_eq!(inventory(&inputs.root), without_actual);
    inputs.write(&actual, &inputs.positive);
    assert_eq!(inventory(&inputs.root), before);
}

#[test]
fn ai2_unmigrated_repo_style_source_has_no_guessing_and_selects_its_exact_nested_path() {
    let inputs = Inputs::new();
    inputs.declare(Some(RECORDS));
    inputs.write(&inputs.root.join(RECORDS), &inputs.positive);
    let actual = inputs.directory.join(RECORDS);
    let before = inventory(&inputs.root);
    let decoy = checked(
        &quality(&inputs.root, &inputs.spec, Some(&inputs.root.join(RECORDS))),
        0,
    );
    assert_eq!(decoy["result"], "statistically_supported");
    let missing = checked(&quality(&inputs.root, &inputs.spec, Some(&actual)), 2);
    assert_eq!(missing["kind"], "parse");
    same(&quality(&inputs.root, &inputs.spec, None), 2, &missing);
    assert_eq!(inventory(&inputs.root), before);
    inputs.write(&actual, &inputs.negative);
    let with_actual = inventory(&inputs.root);
    let expected = checked(&quality(&inputs.root, &inputs.spec, Some(&actual)), 1);
    assert_eq!(expected["result"], "statistically_unsupported");
    same(&quality(&inputs.root, &inputs.spec, None), 1, &expected);
    assert_eq!(inventory(&inputs.root), with_actual);
}

fn larger_positive(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for line in std::str::from_utf8(bytes)
        .expect("JSONL")
        .lines()
        .filter(|l| !l.trim().is_empty())
    {
        let mut record: Value = serde_json::from_str(line).expect("record");
        result.extend(serde_json::to_vec(&record).expect("valid original record"));
        result.push(b'\n');
        record["case_id"] = Value::String(format!(
            "directory-decoy-{}",
            record["case_id"].as_str().expect("case id")
        ));
        result.extend(serde_json::to_vec(&record).expect("extra valid record"));
        result.push(b'\n');
    }
    result
}

#[test]
fn ai2_explicit_records_stay_caller_relative_and_override_local_missing_and_external_sources() {
    let inputs = Inputs::new();
    inputs.write(&inputs.root.join("override.jsonl"), &inputs.negative);
    inputs.write(&inputs.unrelated.join("override.jsonl"), &inputs.positive);
    inputs.write(
        &inputs.directory.join("override.jsonl"),
        &larger_positive(&inputs.positive),
    );
    let directory_decoy = checked(
        &quality(
            &inputs.directory,
            &inputs.spec,
            Some(Path::new("override.jsonl")),
        ),
        0,
    );
    assert_eq!(directory_decoy["result"], "statistically_supported");
    let missing_arg = Path::new("missing-override.jsonl");
    let missing = checked(&quality(&inputs.root, &inputs.spec, Some(missing_arg)), 2);
    assert_eq!(missing["kind"], "parse");
    for declaration in [
        Some(BASENAME),
        Some("absent-declared.jsonl"),
        None,
        Some(EXTERNAL),
    ] {
        inputs.declare(declaration);
        let before = inventory(&inputs.root);
        for (cwd, exit) in [(&inputs.root, 1), (&inputs.unrelated, 0)] {
            let expected = checked(
                &quality(cwd, &inputs.spec, Some(&cwd.join("override.jsonl"))),
                exit,
            );
            assert_ne!(
                expected, directory_decoy,
                "the declaring-directory decoy is observably distinct"
            );
            same(
                &quality(cwd, &inputs.spec, Some(Path::new("override.jsonl"))),
                exit,
                &expected,
            );
        }
        same(
            &quality(&inputs.root, &inputs.spec, Some(missing_arg)),
            2,
            &missing,
        );
        assert_eq!(inventory(&inputs.root), before);
    }
}

#[test]
fn ai2_missing_external_and_selection_refusals_keep_their_existing_precedence() {
    let inputs = Inputs::new();
    let valid = inputs.directory.join(BASENAME);
    let missing = inputs.root.join("absent.jsonl");
    for declaration in [None, Some(EXTERNAL)] {
        inputs.declare(declaration);
        let before = inventory(&inputs.root);
        let refused = checked(&quality(&inputs.root, &inputs.spec, None), 2);
        assert_eq!(refused["kind"], "semantics");
        let message = refused["message"].as_str().expect("refusal message");
        assert!(message.contains("--records"));
        assert!(message.contains(if declaration.is_some() {
            "external"
        } else {
            "no source"
        }));
        let expected = checked(&quality(&inputs.root, &inputs.spec, Some(&valid)), 0);
        assert_eq!(expected["result"], "statistically_supported");
        for (property, dataset) in [(Some("DoesNotExist"), None), (None, Some("SupportEvalV3"))] {
            let selected = checked(
                &eval(&inputs.root, &inputs.spec, Some(&valid), property, dataset),
                2,
            );
            assert_eq!(selected["kind"], "semantics");
            assert!(
                selected["message"]
                    .as_str()
                    .expect("selection message")
                    .contains(if property.is_some() {
                        "unknown statistical_property"
                    } else {
                        "multiple statistical_property"
                    })
            );
            same(
                &eval(
                    &inputs.root,
                    &inputs.spec,
                    Some(&missing),
                    property,
                    dataset,
                ),
                2,
                &selected,
            );
        }
        assert_eq!(inventory(&inputs.root), before);
    }
    let duplicate =
        repository().join("rust/fslc/tests/fixtures/error_envelope_duplicate_ai_project.fsl");
    let error = checked(
        &eval(
            &inputs.root,
            &duplicate,
            Some(&valid),
            Some("DuplicateQuality"),
            None,
        ),
        2,
    );
    assert_eq!(error["kind"], "parse");
    same(
        &eval(
            &inputs.root,
            &duplicate,
            Some(&missing),
            Some("DuplicateQuality"),
            None,
        ),
        2,
        &error,
    );
}

#[test]
fn ai1_documented_positive_example_survives_its_owner_migration_across_cwds() {
    let inputs = Inputs::new();
    let repo = repository();
    let spec = repo.join(EXAMPLE);
    let records = repo.join(RECORDS);
    inputs.write(&inputs.root.join(RECORDS), &inputs.negative);
    let before = inventory(&inputs.root);
    let source_bytes = std::fs::read(&spec).expect("readonly real example");
    let record_bytes = std::fs::read(&records).expect("readonly real records");
    let expected = checked(&quality(&repo, &spec, Some(&records)), 0);
    assert_eq!(expected["result"], "statistically_supported");
    for cwd in [
        repo.as_path(),
        records.parent().expect("example directory"),
        inputs.root.as_path(),
    ] {
        same(&quality(cwd, &spec, None), 0, &expected);
    }
    assert_eq!(
        std::fs::read(&spec).expect("source remains readonly"),
        source_bytes
    );
    assert_eq!(
        std::fs::read(&records).expect("records remain readonly"),
        record_bytes
    );
    assert_eq!(inventory(&inputs.root), before);
}

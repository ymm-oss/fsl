// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fsl_core::KernelModel;
use serde::Deserialize;
use serde_json::{Value, json};

const MARKER: &str = "@fsl.trace ";
const ASSURANCES: [&str; 4] = [
    "source_backed",
    "generated_from_source",
    "generated_only",
    "unknown",
];

#[derive(Debug)]
pub(crate) enum CodeAuditError {
    Io(String),
    Semantics(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeTrace {
    schema: String,
    requirement_id: String,
    kernel_target: String,
    origin_assurance: String,
}

#[derive(Clone, Debug)]
struct LocatedTrace {
    trace: CodeTrace,
    file: String,
    line: usize,
    column: usize,
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), CodeAuditError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| CodeAuditError::Io(format!("{}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(CodeAuditError::Io(format!(
            "code path is not a file or directory: {}",
            path.display()
        )));
    }
    for entry in std::fs::read_dir(path)
        .map_err(|error| CodeAuditError::Io(format!("{}: {error}", path.display())))?
    {
        let entry = entry.map_err(|error| CodeAuditError::Io(error.to_string()))?;
        if entry.file_name() == ".git" {
            continue;
        }
        collect_files(&entry.path(), files)?;
    }
    Ok(())
}

fn parse_file(path: &Path, traces: &mut Vec<LocatedTrace>) -> Result<(), CodeAuditError> {
    let bytes = std::fs::read(path)
        .map_err(|error| CodeAuditError::Io(format!("{}: {error}", path.display())))?;
    for (line_index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let Some(marker_index) = line
            .windows(MARKER.len())
            .position(|window| window == MARKER.as_bytes())
        else {
            continue;
        };
        let text = std::str::from_utf8(line).map_err(|_| {
            CodeAuditError::Semantics(format!(
                "{}:{}: code trace marker line is not UTF-8",
                crate::analysis_display_path(path),
                line_index + 1
            ))
        })?;
        let json_text = &text[marker_index + MARKER.len()..];
        let trace: CodeTrace = serde_json::from_str(json_text).map_err(|error| {
            CodeAuditError::Semantics(format!(
                "{}:{}: malformed code trace annotation: {error}",
                crate::analysis_display_path(path),
                line_index + 1
            ))
        })?;
        if trace.schema != "fsl-code-trace.v0" {
            return Err(CodeAuditError::Semantics(format!(
                "{}:{}: unsupported code trace schema '{}'",
                crate::analysis_display_path(path),
                line_index + 1,
                trace.schema
            )));
        }
        if trace.requirement_id.is_empty() || trace.kernel_target.is_empty() {
            return Err(CodeAuditError::Semantics(format!(
                "{}:{}: requirement_id and kernel_target must be non-empty",
                crate::analysis_display_path(path),
                line_index + 1
            )));
        }
        if !ASSURANCES.contains(&trace.origin_assurance.as_str()) {
            return Err(CodeAuditError::Semantics(format!(
                "{}:{}: unsupported origin_assurance '{}'",
                crate::analysis_display_path(path),
                line_index + 1,
                trace.origin_assurance
            )));
        }
        traces.push(LocatedTrace {
            trace,
            file: crate::analysis_display_path(path),
            line: line_index + 1,
            column: text[..marker_index].chars().count() + 1,
        });
    }
    Ok(())
}

fn location(trace: &LocatedTrace) -> Value {
    json!({
        "file":trace.file,
        "line":trace.line,
        "column":trace.column,
        "origin_assurance":trace.trace.origin_assurance,
    })
}

type RequirementTargets = BTreeMap<String, BTreeSet<String>>;
type Implementations<'a> = BTreeMap<(String, String), Vec<&'a LocatedTrace>>;

fn expected_targets(model: &KernelModel) -> (RequirementTargets, BTreeSet<(String, String)>) {
    let mut expected = RequirementTargets::new();
    for (target, links) in model.requirement_targets() {
        for link in links {
            expected.entry(link.id).or_default().insert(target.clone());
        }
    }
    let pairs = expected
        .iter()
        .flat_map(|(requirement, targets)| {
            targets
                .iter()
                .map(move |target| (requirement.clone(), target.clone()))
        })
        .collect();
    (expected, pairs)
}

fn classify<'a>(
    traces: &'a [LocatedTrace],
    expected: &RequirementTargets,
    expected_pairs: &BTreeSet<(String, String)>,
) -> (Implementations<'a>, Vec<Value>) {
    let mut implementations = Implementations::new();
    let mut findings = Vec::new();
    for trace in traces {
        let requirement = &trace.trace.requirement_id;
        let target = &trace.trace.kernel_target;
        let finding = if !expected.contains_key(requirement) {
            Some(json!({
                "finding_type":"orphan_code_annotation",
                "severity":"review_required",
                "formal_status":"not_a_violation",
                "requirement_id":requirement,
                "kernel_target":target,
                "location":location(trace),
            }))
        } else if !expected_pairs.contains(&(requirement.clone(), target.clone())) {
            Some(json!({
                "finding_type":"annotation_target_mismatch",
                "severity":"review_required",
                "formal_status":"not_a_violation",
                "requirement_id":requirement,
                "kernel_target":target,
                "expected_kernel_targets":expected[requirement],
                "location":location(trace),
            }))
        } else {
            implementations
                .entry((requirement.clone(), target.clone()))
                .or_default()
                .push(trace);
            None
        };
        findings.extend(finding);
    }
    for (requirement, target) in expected_pairs {
        if !implementations.contains_key(&(requirement.clone(), target.clone())) {
            findings.push(json!({
                "finding_type":"missing_requirement_implementation",
                "severity":"review_required",
                "formal_status":"not_a_violation",
                "requirement_id":requirement,
                "kernel_target":target,
            }));
        }
    }
    findings.sort_by_key(|finding| {
        (
            finding["requirement_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            finding["kernel_target"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            finding["location"]["file"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            finding["location"]["line"].as_u64().unwrap_or_default(),
            finding["location"]["column"].as_u64().unwrap_or_default(),
            finding["finding_type"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    });
    (implementations, findings)
}

fn requirement_rows(
    expected: &RequirementTargets,
    implementations: &Implementations<'_>,
) -> (Vec<Value>, usize, usize) {
    let mut covered_requirements = 0;
    let mut partial_requirements = 0;
    let rows = expected
        .iter()
        .map(|(requirement, targets)| {
            let kernel_targets = targets
                .iter()
                .map(|target| {
                    let locations = implementations
                        .get(&(requirement.clone(), target.clone()))
                        .map(|traces| {
                            traces
                                .iter()
                                .map(|trace| location(trace))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    json!({
                        "kernel_target":target,
                        "status":if locations.is_empty() { "missing" } else { "covered" },
                        "implementations":locations,
                    })
                })
                .collect::<Vec<_>>();
            let covered_targets = kernel_targets
                .iter()
                .filter(|target| target["status"] == "covered")
                .count();
            let status = if covered_targets == targets.len() {
                covered_requirements += 1;
                "covered"
            } else if covered_targets == 0 {
                "missing"
            } else {
                partial_requirements += 1;
                "partial"
            };
            json!({
                "requirement_id":requirement,
                "status":status,
                "kernel_targets":kernel_targets,
            })
        })
        .collect();
    (rows, covered_requirements, partial_requirements)
}

fn assurance_coverage(
    traces: &[LocatedTrace],
    implementations: &Implementations<'_>,
) -> serde_json::Map<String, Value> {
    ASSURANCES
        .into_iter()
        .map(|name| {
            let matching = implementations
                .values()
                .filter(|traces| {
                    traces
                        .iter()
                        .any(|trace| trace.trace.origin_assurance == name)
                })
                .count();
            let annotations = traces
                .iter()
                .filter(|trace| trace.trace.origin_assurance == name)
                .count();
            (
                name.to_owned(),
                json!({"annotations":annotations,"requirement_targets":matching}),
            )
        })
        .collect()
}

pub(crate) fn analyze(model: &KernelModel, code_path: &Path) -> Result<Value, CodeAuditError> {
    let mut files = Vec::new();
    collect_files(code_path, &mut files)?;
    files.sort_by_key(|path| crate::analysis_display_path(path));

    let mut traces = Vec::new();
    for file in &files {
        parse_file(file, &mut traces)?;
    }
    traces.sort_by_key(|trace| {
        (
            trace.file.clone(),
            trace.line,
            trace.column,
            trace.trace.requirement_id.clone(),
            trace.trace.kernel_target.clone(),
        )
    });
    traces.dedup_by(|left, right| {
        left.file == right.file
            && left.line == right.line
            && left.column == right.column
            && left.trace.requirement_id == right.trace.requirement_id
            && left.trace.kernel_target == right.trace.kernel_target
            && left.trace.origin_assurance == right.trace.origin_assurance
    });

    let (expected, expected_pairs) = expected_targets(model);
    let (implementations, findings) = classify(&traces, &expected, &expected_pairs);
    let (requirement_rows, covered_requirements, partial_requirements) =
        requirement_rows(&expected, &implementations);
    let covered_pairs = implementations.len();
    let assurance = assurance_coverage(&traces, &implementations);
    Ok(json!({
        "analysis":"structure",
        "projection":"code_audit",
        "schema_version":"code-audit.v0",
        "formal_status":"not_a_violation",
        "code_path":crate::analysis_display_path(code_path),
        "files_scanned":files.len(),
        "requirements":requirement_rows,
        "coverage":{
            "requirements":{
                "total":expected.len(),
                "covered":covered_requirements,
                "partial":partial_requirements,
                "missing":expected.len() - covered_requirements - partial_requirements,
            },
            "requirement_targets":{
                "total":expected_pairs.len(),
                "covered":covered_pairs,
                "missing":expected_pairs.len() - covered_pairs,
            },
            "by_origin_assurance":assurance,
        },
        "findings":findings,
    }))
}

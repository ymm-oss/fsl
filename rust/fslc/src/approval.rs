// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Versioned approval records that bind reviewed artifacts to normalized FSL.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use fsl_core::KernelModel;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub const APPROVAL_SCHEMA: &str = "fslc.approval.v1";
pub const APPROVAL_SCHEMA_V2: &str = "fslc.approval.v2";
pub const SIGNATURE_ALGORITHM: &str = "ed25519";
pub const SPEC_DIGEST_ALGORITHM: &str = "fsl-kernel-ast-v1+sha256";
pub const ARTIFACT_DIGEST_ALGORITHM: &str = "fsl-rendered-artifact-v1+sha256";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationInputs {
    pub depth: usize,
    pub deadlock: String,
    pub engine: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecBinding {
    pub path: String,
    pub digest_algorithm: String,
    pub digest: String,
    pub git_commit: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetBinding {
    pub kind: String,
    pub path: String,
    pub digest_algorithm: String,
    pub digest: String,
    pub generator: String,
    pub generator_version: String,
    pub inputs: GenerationInputs,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalMetadata {
    pub approver: String,
    pub approved_at: String,
    pub requirements: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRecord {
    pub schema: String,
    pub spec: SpecBinding,
    pub target: TargetBinding,
    pub approval: ApprovalMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetachedSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRecordV2 {
    pub schema: String,
    pub spec: SpecBinding,
    pub target: TargetBinding,
    pub approval: ApprovalMetadata,
    pub signature: DetachedSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VersionedApprovalRecord {
    V1(ApprovalRecord),
    V2(ApprovalRecordV2),
}

impl VersionedApprovalRecord {
    #[must_use]
    pub fn binding(&self) -> ApprovalRecord {
        match self {
            Self::V1(record) => record.clone(),
            Self::V2(record) => ApprovalRecord {
                schema: APPROVAL_SCHEMA.to_owned(),
                spec: record.spec.clone(),
                target: record.target.clone(),
                approval: record.approval.clone(),
            },
        }
    }
}

#[derive(Default)]
pub struct TrustStore {
    keys: BTreeMap<String, VerifyingKey>,
}

impl TrustStore {
    pub fn load(paths: &[PathBuf]) -> Result<Self, String> {
        let mut keys = BTreeMap::new();
        for path in paths {
            let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
            let key = VerifyingKey::from_public_key_pem(&source).map_err(|error| {
                format!("invalid Ed25519 public key '{}': {error}", path.display())
            })?;
            keys.insert(key_id(&key), key);
        }
        Ok(Self { keys })
    }

    pub fn verify(&self, record: &ApprovalRecordV2) -> Result<bool, String> {
        let key = self.keys.get(&record.signature.key_id).ok_or_else(|| {
            format!(
                "no trusted Ed25519 public key matches '{}'",
                record.signature.key_id
            )
        })?;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&record.signature.value)
            .map_err(|error| format!("invalid detached signature encoding: {error}"))?;
        if base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw) != record.signature.value {
            return Err("detached signature is not canonical base64url-no-pad".to_owned());
        }
        let signature = Signature::from_slice(&raw)
            .map_err(|error| format!("invalid Ed25519 signature: {error}"))?;
        Ok(key
            .verify_strict(&signature_payload(record)?, &signature)
            .is_ok())
    }
}

fn is_location(value: &Map<String, Value>) -> bool {
    value.len() == 2
        && value.get("line").is_some_and(Value::is_number)
        && value.get("column").is_some_and(Value::is_number)
}

fn normalized_ast(value: &Value) -> Option<Value> {
    match value {
        Value::Array(items) => Some(Value::Array(
            items.iter().filter_map(normalized_ast).collect(),
        )),
        Value::Object(items) if is_location(items) => None,
        Value::Object(items) => {
            let mut keys = items.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut normalized = Map::new();
            for key in keys {
                if let Some(value) = normalized_ast(&items[key]) {
                    normalized.insert(key.clone(), value);
                }
            }
            Some(Value::Object(normalized))
        }
        _ => Some(value.clone()),
    }
}

#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn stable_json(value: &Value, strip_execution_metadata: bool) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| stable_json(item, false)).collect())
        }
        Value::Object(items) => {
            let mut keys = items
                .keys()
                .filter(|key| {
                    !strip_execution_metadata || !matches!(key.as_str(), "cost" | "cache")
                })
                .collect::<Vec<_>>();
            keys.sort_unstable();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), stable_json(&items[key], false)))
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

fn key_id(key: &VerifyingKey) -> String {
    sha256_bytes(key.as_bytes())
}

fn signature_payload(record: &ApprovalRecordV2) -> Result<Vec<u8>, String> {
    let mut value = serde_json::to_value(record).map_err(|error| error.to_string())?;
    value
        .get_mut("signature")
        .and_then(Value::as_object_mut)
        .expect("serialized v2 signature is an object")
        .remove("value");
    let mut payload = APPROVAL_SCHEMA_V2.as_bytes().to_vec();
    payload.push(0);
    payload.extend(
        serde_json::to_vec(&stable_json(&value, false)).map_err(|error| error.to_string())?,
    );
    Ok(payload)
}

/// Remove execution-only noise before binding a human-facing artifact.
pub fn normalized_artifact(kind: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if kind == "scenarios" {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("scenario artifact is not valid JSON: {error}"))?;
        return serde_json::to_vec(&stable_json(&value, true)).map_err(|error| error.to_string());
    }
    if kind == "html" {
        let source = std::str::from_utf8(bytes)
            .map_err(|error| format!("HTML artifact is not UTF-8: {error}"))?;
        let normalized = normalize_verify_cost(
            source,
            "&quot;",
            "&lt;elapsed&gt;",
            "&quot;&lt;metric&gt;&quot;",
        );
        let normalized = normalize_verify_cost(&normalized, "\"", "\"<elapsed>\"", "\"<metric>\"");
        let normalized =
            normalize_number_key(&normalized, "&quot;", "elapsed_s", "&lt;elapsed&gt;");
        let normalized = normalize_number_key(&normalized, "\"", "elapsed_s", "\"<elapsed>\"");
        return Ok(normalized.into_bytes());
    }
    Ok(bytes.to_vec())
}

fn normalize_verify_cost(
    source: &str,
    quote: &str,
    elapsed_replacement: &str,
    metric_replacement: &str,
) -> String {
    const VERIFY_JSON: &str = "<details><summary>verify JSON</summary>";
    const PRE: &str = "<div class=\"code-block\"><pre>";
    const PRE_END: &str = "</pre>";

    let Some(section_start) = source.find(VERIFY_JSON) else {
        return source.to_owned();
    };
    let Some(relative_pre_start) = source[section_start..].find(PRE) else {
        return source.to_owned();
    };
    let json_start = section_start + relative_pre_start + PRE.len();
    let Some(relative_json_end) = source[json_start..].find(PRE_END) else {
        return source.to_owned();
    };
    let json_end = json_start + relative_json_end;
    let json = &source[json_start..json_end];
    let marker = format!("\n  {quote}cost{quote}: {{");
    let Some(cost_start) = json.find(&marker) else {
        return source.to_owned();
    };
    let cost_start = cost_start + 1;
    let Some(cost_end) = cost_block_end(&json[cost_start..]) else {
        return source.to_owned();
    };
    let cost_end = cost_start + cost_end;
    let normalized_cost = normalize_performance_numbers(
        &json[cost_start..cost_end],
        quote,
        elapsed_replacement,
        metric_replacement,
    );
    format!(
        "{}{}{}{}{}",
        &source[..json_start],
        &json[..cost_start],
        normalized_cost,
        &json[cost_end..],
        &source[json_end..]
    )
}

fn cost_block_end(text: &str) -> Option<usize> {
    let mut depth = 0_i64;
    for (index, byte) in text.bytes().enumerate() {
        depth += match byte {
            b'{' => 1,
            b'}' => -1,
            _ => 0,
        };
        if depth == 0 && byte == b'}' {
            return Some(index + 1);
        }
    }
    None
}

fn normalize_performance_numbers(
    line: &str,
    quote: &str,
    elapsed_replacement: &str,
    metric_replacement: &str,
) -> String {
    let mut normalized = line.to_owned();
    for key in [
        "elapsed_s",
        "check_elapsed_s",
        "conflicts",
        "decisions",
        "propagations",
        "memory_mb",
    ] {
        let replacement = if key == "elapsed_s" {
            elapsed_replacement
        } else {
            metric_replacement
        };
        normalized = normalize_number_key(&normalized, quote, key, replacement);
    }
    normalized
}

fn normalize_number_key(source: &str, quote: &str, key: &str, replacement: &str) -> String {
    let marker = format!("{quote}{key}{quote}:");
    let mut normalized = source.to_owned();
    let mut cursor = 0;
    while let Some(relative_start) = normalized[cursor..].find(&marker) {
        let marker_end = cursor + relative_start + marker.len();
        let value_start = marker_end
            + normalized[marker_end..]
                .len()
                .saturating_sub(normalized[marker_end..].trim_start().len());
        let value_len = normalized[value_start..]
            .bytes()
            .take_while(|byte| matches!(byte, b'0'..=b'9' | b'.' | b'-' | b'+' | b'e' | b'E'))
            .count();
        if value_len == 0 {
            cursor = marker_end;
            continue;
        }
        normalized.replace_range(value_start..value_start + value_len, replacement);
        cursor = value_start + replacement.len();
    }
    normalized
}

/// Hash the fully lowered kernel AST while ignoring source locations.
///
/// Comments, whitespace, source paths, and line movement do not invalidate an
/// approval. Imported/composed specifications are covered because the resolver
/// expands them before `python_ast` is projected.
pub fn spec_digest(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let resolver = fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
    let kernel =
        fsl_core::parse_kernel_source(&source, &resolver).map_err(|error| error.to_string())?;
    let ast = normalized_ast(&kernel.python_ast())
        .ok_or_else(|| "normalized kernel AST is empty".to_owned())?;
    let encoded = serde_json::to_vec(&ast).map_err(|error| error.to_string())?;
    let mut framed = SPEC_DIGEST_ALGORITHM.as_bytes().to_vec();
    framed.push(0);
    framed.extend(encoded);
    Ok(sha256_bytes(&framed))
}

#[must_use]
pub fn requirement_ids(model: &KernelModel) -> Vec<String> {
    model
        .requirement_targets()
        .into_values()
        .flatten()
        .map(|requirement| requirement.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn git_output(cwd: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Return `(repository root, repository-relative spec path, HEAD commit)`.
pub fn git_location(path: &Path) -> Result<(PathBuf, String, String), String> {
    let absolute = path.canonicalize().map_err(|error| error.to_string())?;
    let root = PathBuf::from(git_output(
        absolute.parent().unwrap_or_else(|| Path::new(".")),
        &["rev-parse", "--show-toplevel"],
    )?)
    .canonicalize()
    .map_err(|error| error.to_string())?;
    let relative = absolute
        .strip_prefix(&root)
        .map_err(|_| "spec is outside its Git repository".to_owned())?
        .to_string_lossy()
        .replace('\\', "/");
    git_output(&root, &["ls-files", "--error-unmatch", "--", &relative])?;
    let commit = git_output(&root, &["rev-parse", "HEAD"])?;
    Ok((root, relative, commit))
}

/// Resolve the Git baseline and require all tracked content to match it.
pub fn git_binding(path: &Path) -> Result<(PathBuf, String, String), String> {
    let (root, relative, commit) = git_location(path)?;
    let status = Command::new("git")
        .args(["diff", "--quiet", "HEAD", "--"])
        .current_dir(&root)
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(
            "approval creation requires a clean tracked worktree so the baseline can be reconstructed"
                .to_owned(),
        );
    }
    Ok((root, relative, commit))
}

/// Require the recorded commit and its specification path to exist locally.
pub fn verify_git_baseline(repo: &Path, relative_path: &str, commit: &str) -> Result<(), String> {
    let commit_object = format!("{commit}^{{commit}}");
    git_output(repo, &["cat-file", "-e", &commit_object])?;
    let spec_object = format!("{commit}:{relative_path}");
    git_output(repo, &["cat-file", "-e", &spec_object])?;
    Ok(())
}

pub fn read_record(path: &Path) -> Result<ApprovalRecord, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let record: ApprovalRecord = serde_json::from_str(&source)
        .map_err(|error| format!("invalid approval record: {error}"))?;
    validate_record(&record)?;
    Ok(record)
}

pub fn read_versioned_record(path: &Path) -> Result<VersionedApprovalRecord, String> {
    #[derive(Deserialize)]
    struct SchemaHeader {
        schema: String,
    }

    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let header: SchemaHeader = serde_json::from_str(&source)
        .map_err(|error| format!("invalid approval record: {error}"))?;
    match header.schema.as_str() {
        APPROVAL_SCHEMA => read_record(path).map(VersionedApprovalRecord::V1),
        APPROVAL_SCHEMA_V2 => {
            let record: ApprovalRecordV2 = serde_json::from_str(&source)
                .map_err(|error| format!("invalid approval record: {error}"))?;
            validate_record_v2(&record)?;
            Ok(VersionedApprovalRecord::V2(record))
        }
        schema => Err(format!("unsupported approval schema '{schema}'")),
    }
}

pub fn validate_record(record: &ApprovalRecord) -> Result<(), String> {
    if record.schema != APPROVAL_SCHEMA {
        return Err(format!("unsupported approval schema '{}'", record.schema));
    }
    if record.spec.digest_algorithm != SPEC_DIGEST_ALGORITHM {
        return Err(format!(
            "unsupported spec digest algorithm '{}'",
            record.spec.digest_algorithm
        ));
    }
    if record.target.digest_algorithm != ARTIFACT_DIGEST_ALGORITHM {
        return Err(format!(
            "unsupported artifact digest algorithm '{}'",
            record.target.digest_algorithm
        ));
    }
    if !matches!(record.target.kind.as_str(), "ledger" | "html" | "scenarios") {
        return Err(format!(
            "unsupported approval target '{}'",
            record.target.kind
        ));
    }
    if record.spec.path.trim().is_empty() || record.target.path.trim().is_empty() {
        return Err("approval record paths must not be empty".to_owned());
    }
    if !is_sha256(&record.spec.digest) || !is_sha256(&record.target.digest) {
        return Err("approval record contains an invalid SHA-256 digest".to_owned());
    }
    if !matches!(record.spec.git_commit.len(), 40 | 64)
        || !record
            .spec
            .git_commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("approval record contains an invalid Git commit".to_owned());
    }
    if record.target.generator != "fslc" || record.target.generator_version.trim().is_empty() {
        return Err("approval record contains an unsupported generator".to_owned());
    }
    if !matches!(
        record.target.inputs.deadlock.as_str(),
        "warn" | "error" | "ignore"
    ) || !matches!(record.target.inputs.engine.as_str(), "bmc" | "induction")
    {
        return Err("approval record contains invalid generation inputs".to_owned());
    }
    if record.approval.approver.trim().is_empty() {
        return Err("approval record requires a non-empty approver".to_owned());
    }
    if !is_canonical_utc_timestamp(&record.approval.approved_at) {
        return Err("approval record requires a canonical UTC approval timestamp".to_owned());
    }
    if record.approval.requirements.is_empty() {
        return Err("approval record requires at least one requirement ID".to_owned());
    }
    let mut requirements = BTreeSet::new();
    if record
        .approval
        .requirements
        .iter()
        .any(|requirement| requirement.trim().is_empty() || !requirements.insert(requirement))
    {
        return Err("approval record requirement IDs must be non-empty and unique".to_owned());
    }
    Ok(())
}

pub fn validate_record_v2(record: &ApprovalRecordV2) -> Result<(), String> {
    if record.schema != APPROVAL_SCHEMA_V2 {
        return Err(format!("unsupported approval schema '{}'", record.schema));
    }
    validate_record(&ApprovalRecord {
        schema: APPROVAL_SCHEMA.to_owned(),
        spec: record.spec.clone(),
        target: record.target.clone(),
        approval: record.approval.clone(),
    })?;
    if record.signature.algorithm != SIGNATURE_ALGORITHM {
        return Err(format!(
            "unsupported approval signature algorithm '{}'",
            record.signature.algorithm
        ));
    }
    if !is_sha256(&record.signature.key_id) {
        return Err("approval record contains an invalid signature key ID".to_owned());
    }
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&record.signature.value)
        .map_err(|error| format!("invalid detached signature encoding: {error}"))?;
    if raw.len() != Signature::BYTE_SIZE
        || base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw) != record.signature.value
    {
        return Err("approval record contains an invalid detached signature".to_owned());
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_canonical_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return false;
    }
    let number = |start: usize, end: usize| {
        bytes[start..end].iter().try_fold(0_u32, |value, byte| {
            byte.is_ascii_digit()
                .then_some(value * 10 + u32::from(*byte - b'0'))
        })
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0, 4),
        number(5, 7),
        number(8, 10),
        number(11, 13),
        number(14, 16),
        number(17, 19),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day) && hour < 24 && minute < 60 && second < 60
}

pub fn write_record(path: &Path, record: &ApprovalRecord) -> Result<(), String> {
    let mut encoded = serde_json::to_string_pretty(record).map_err(|error| error.to_string())?;
    encoded.push('\n');
    std::fs::write(path, encoded).map_err(|error| error.to_string())
}

pub fn sign_record(record: ApprovalRecord, private_key: &Path) -> Result<ApprovalRecordV2, String> {
    validate_record(&record)?;
    let source = std::fs::read_to_string(private_key).map_err(|error| error.to_string())?;
    let key = SigningKey::from_pkcs8_pem(&source).map_err(|error| {
        format!(
            "invalid Ed25519 private key '{}': {error}",
            private_key.display()
        )
    })?;
    let mut signed = ApprovalRecordV2 {
        schema: APPROVAL_SCHEMA_V2.to_owned(),
        spec: record.spec,
        target: record.target,
        approval: record.approval,
        signature: DetachedSignature {
            algorithm: SIGNATURE_ALGORITHM.to_owned(),
            key_id: key_id(&key.verifying_key()),
            value: String::new(),
        },
    };
    let signature = key.sign(&signature_payload(&signed)?);
    signed.signature.value =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());
    Ok(signed)
}

pub fn write_record_v2(path: &Path, record: &ApprovalRecordV2) -> Result<(), String> {
    validate_record_v2(record)?;
    let mut encoded = serde_json::to_string_pretty(record).map_err(|error| error.to_string())?;
    encoded.push('\n');
    std::fs::write(path, encoded).map_err(|error| error.to_string())
}

#[must_use]
pub fn evaluate(
    record: &ApprovalRecord,
    record_path: &Path,
    current_spec_digest: &str,
    current_artifact_digest: &str,
    current_generator_version: &str,
) -> Value {
    let mut reasons = Vec::new();
    if record.spec.digest != current_spec_digest {
        reasons.push("spec_changed");
    }
    if record.target.digest != current_artifact_digest {
        reasons.push("rendering_changed");
    }
    if record.target.generator_version != current_generator_version {
        reasons.push("renderer_changed");
    }
    let status = if reasons.is_empty() {
        "approved"
    } else {
        "drifted"
    };
    json!({
        "status": status,
        "reasons": reasons,
        "record": record_path.display().to_string(),
        "target_kind": record.target.kind,
        "approver": record.approval.approver,
        "approved_at": record.approval.approved_at,
        "requirements": record.approval.requirements,
        "baseline_digest": record.spec.digest,
        "current_digest": current_spec_digest,
        "artifact_digest": record.target.digest,
        "current_artifact_digest": current_artifact_digest,
        "diff_base": record.spec.git_commit,
        "semantic_diff_command": format!(
            "fslc approval diff {} --record {}",
            shell_word(&record.spec.path),
            shell_word(&record_path.display().to_string())
        ),
    })
}

fn shell_word(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[must_use]
pub fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    let day_seconds = seconds % 86_400;
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;

    // Howard Hinnant's civil-from-days transform, with day zero at Unix epoch.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::{
        is_canonical_utc_timestamp, normalized_artifact, normalized_ast, now_rfc3339,
        requirement_ids, sha256_bytes, shell_word,
    };
    use fsl_core::{Annotation, FsResolver, action_target, build_model, parse_kernel_source};
    use fsl_syntax::{SourcePos, Span};
    use serde_json::{Value, json};

    #[test]
    fn normalized_ast_removes_locations_but_preserves_metadata() {
        let value = json!([
            "action",
            {"line": 4, "column": 2},
            {"id": "REQ-1", "text": "approved"},
            ["call", "next", [], {"line": 8, "column": 7}],
        ]);
        assert_eq!(
            normalized_ast(&value),
            Some(json!([
                "action",
                {"id": "REQ-1", "text": "approved"},
                ["call", "next", []],
            ]))
        );
    }

    #[test]
    fn digest_is_prefixed_and_timestamp_is_utc() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let now = now_rfc3339();
        assert!(now.ends_with('Z'));
        assert_eq!(now.len(), 20);
        assert!(is_canonical_utc_timestamp(&now));
        assert!(!is_canonical_utc_timestamp("2026-02-30T12:00:00Z"));
    }

    #[test]
    fn scenario_normalization_keeps_domain_cost_and_shell_quotes_paths() {
        let artifact = json!({
            "fsl": "1.0",
            "cost": {"elapsed_s": 0.25},
            "cache": {"hit": true},
            "scenarios": [{"initial_state": {"cost": 7, "cache": false}}],
        });
        let normalized = normalized_artifact(
            "scenarios",
            &serde_json::to_vec(&artifact).expect("serialize scenario"),
        )
        .expect("normalize scenario");
        let normalized: Value = serde_json::from_slice(&normalized).expect("normalized JSON");
        assert!(normalized.get("cost").is_none());
        assert!(normalized.get("cache").is_none());
        assert_eq!(normalized["scenarios"][0]["initial_state"]["cost"], 7);
        assert_eq!(normalized["scenarios"][0]["initial_state"]["cache"], false);
        assert_eq!(shell_word("specs/order.fsl"), "specs/order.fsl");
        assert_eq!(
            shell_word("review specs/order.fsl"),
            "'review specs/order.fsl'"
        );
    }

    #[test]
    fn html_normalization_removes_every_performance_measurement() {
        let first = br#"<details><summary>verify JSON</summary><div><div class="code-block"><pre>{
  "cost": {"elapsed_s":0.1,"solver":{"checks":3,"check_elapsed_s":0.02,"conflicts":4,"decisions":5,"propagations":6,"memory_mb":18.2},"properties":[{"kind":"invariant","name":"Safe","checks":2,"elapsed_s":0.01}]},
  "state": {"cost":{"conflicts":1},"violation":{"cost":{"elapsed_s":0.03}}}
}</pre></div></div></details>"#;
        let second = br#"<details><summary>verify JSON</summary><div><div class="code-block"><pre>{
  "cost": {"elapsed_s":9.1,"solver":{"checks":3,"check_elapsed_s":7.02,"conflicts":40,"decisions":50,"propagations":60,"memory_mb":81.2},"properties":[{"kind":"invariant","name":"Safe","checks":2,"elapsed_s":7.01}]},
  "state": {"cost":{"conflicts":1},"violation":{"cost":{"elapsed_s":8.03}}}
}</pre></div></div></details>"#;
        let domain_change = std::str::from_utf8(second)
            .expect("HTML fixture is UTF-8")
            .replace("\"cost\":{\"conflicts\":1}", "\"cost\":{\"conflicts\":2}");

        assert_eq!(
            normalized_artifact("html", first).expect("normalize first HTML"),
            normalized_artifact("html", second).expect("normalize second HTML")
        );
        assert_ne!(
            normalized_artifact("html", second).expect("normalize unchanged domain HTML"),
            normalized_artifact("html", domain_change.as_bytes())
                .expect("normalize changed domain HTML")
        );

        assert_eq!(
            normalized_artifact(
                "html",
                b"<details><summary>verify JSON</summary><div><div class=\"code-block\"><pre>{\n  \"cost\": {\"elapsed_s\":0.25}\n}</pre></div></div></details>",
            )
            .expect("normalize legacy HTML"),
            b"<details><summary>verify JSON</summary><div><div class=\"code-block\"><pre>{\n  \"cost\": {\"elapsed_s\":\"<elapsed>\"}\n}</pre></div></div></details>"
        );
        assert_eq!(
            normalized_artifact(
                "html",
                b"<details><summary>verify JSON</summary><div><div class=\"code-block\"><pre>{\n  &quot;cost&quot;: {&quot;elapsed_s&quot;:0.25}\n}</pre></div></div></details>",
            )
            .expect("normalize escaped legacy HTML"),
            b"<details><summary>verify JSON</summary><div><div class=\"code-block\"><pre>{\n  &quot;cost&quot;: {&quot;elapsed_s&quot;:&lt;elapsed&gt;}\n}</pre></div></div></details>"
        );
    }

    #[test]
    fn approval_requirement_ids_include_every_typed_relation() {
        let mut kernel = parse_kernel_source(
            r#"
spec ApprovalAnnotations {
  state { ready: Bool }
  init { ready = false }
  action publish() "REQ-2: second" { ready = true }
}
"#,
            &FsResolver::new("."),
        )
        .expect("parse spec");
        let position = SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        };
        kernel.bind_annotation(
            action_target("publish"),
            Annotation::Requirement {
                id: "REQ-1".to_owned(),
                text: Some("first".to_owned()),
                span: Span {
                    start: position,
                    end: position,
                },
            },
        );
        let model = build_model(kernel).expect("build model");
        assert_eq!(requirement_ids(&model), ["REQ-1", "REQ-2"]);
    }
}

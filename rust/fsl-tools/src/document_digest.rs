// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Digest primitives for the Requirement Claim IR (RCIR), issue #325.
//!
//! The framing follows `fslc::approval`'s convention exactly
//! (`sha256(algorithm_name || 0x00 || canonical_json)`, rendered as
//! `"sha256:<64 hex>"`) so a future approval integration (issue #333) can join
//! on the same `spec_digest` identity. The algorithm name is reused verbatim;
//! the byte-framing implementation here is intentionally independent from
//! `fslc::approval`'s (rather than a cross-crate refactor of already-shipped,
//! tested code) to keep this issue's change small and low-risk.

use fsl_core::KernelSpec;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const SPEC_DIGEST_ALGORITHM: &str = "fsl-kernel-ast-v1+sha256";
pub const CLAIM_SET_DIGEST_ALGORITHM: &str = "fsl-rcir-claim-set-v1+sha256";
pub const CLAIM_DIGEST_ALGORITHM: &str = "fsl-rcir-claim-v1+sha256";
/// Digests a generated document's own claim-block *text* (issue #329) —
/// distinct from [`CLAIM_DIGEST_ALGORITHM`], which digests a claim's checked
/// semantics and never sees rendered prose.
pub const CLAIM_BLOCK_DIGEST_ALGORITHM: &str = "fsl-doc-claim-block-v1+sha256";

#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn is_location(value: &Map<String, Value>) -> bool {
    value.len() == 2
        && value.get("line").is_some_and(Value::is_number)
        && value.get("column").is_some_and(Value::is_number)
}

/// Strip embedded source-location objects and recursively sort object keys.
///
/// Mirrors `fslc::approval::normalized_ast` byte-for-byte so the two
/// producers cannot silently diverge on what counts as a semantic change.
#[must_use]
pub fn normalized_kernel_ast(value: &Value) -> Option<Value> {
    match value {
        Value::Array(items) => Some(Value::Array(
            items.iter().filter_map(normalized_kernel_ast).collect(),
        )),
        Value::Object(items) if is_location(items) => None,
        Value::Object(items) => {
            let mut keys = items.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut normalized = Map::new();
            for key in keys {
                if let Some(value) = normalized_kernel_ast(&items[key]) {
                    normalized.insert(key.clone(), value);
                }
            }
            Some(Value::Object(normalized))
        }
        _ => Some(value.clone()),
    }
}

/// Recursively sort object keys without stripping anything.
///
/// The workspace enables `serde_json` `preserve_order`, so canonicalization
/// requires an explicit sort rather than relying on a `BTreeMap`-backed map.
#[must_use]
pub fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        Value::Object(items) => {
            let mut keys = items.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical_value(&items[key])))
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

/// `sha256(algorithm || 0x00 || compact_json(canonical_value(value)))`.
#[must_use]
pub fn framed_digest(algorithm: &str, value: &Value) -> String {
    let encoded =
        serde_json::to_vec(&canonical_value(value)).expect("canonical JSON value serializes");
    let mut framed = algorithm.as_bytes().to_vec();
    framed.push(0);
    framed.extend(encoded);
    sha256_bytes(&framed)
}

/// `sha256(algorithm || 0x00 || utf8(text))`, framed like [`framed_digest`]
/// but over raw text bytes rather than canonical JSON — used to digest a
/// generated document's rendered claim-block text (issue #329), where the
/// input is prose, not an AST value.
#[must_use]
pub fn framed_text_digest(algorithm: &str, text: &str) -> String {
    let mut framed = algorithm.as_bytes().to_vec();
    framed.push(0);
    framed.extend(text.as_bytes());
    sha256_bytes(&framed)
}

/// The same `spec_digest` identity `fslc approval` binds to (issue #333 joins
/// on this), computed from an already-parsed [`KernelSpec`] with no file I/O.
///
/// # Errors
///
/// Returns an error string when the normalized kernel AST is empty.
pub fn spec_digest_from_kernel(kernel: &KernelSpec) -> Result<String, String> {
    let ast = normalized_kernel_ast(&kernel.python_ast())
        .ok_or_else(|| "normalized kernel AST is empty".to_owned())?;
    Ok(framed_digest(SPEC_DIGEST_ALGORITHM, &ast))
}

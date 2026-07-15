// SPDX-License-Identifier: Apache-2.0

//! Shared fail-closed readers for the versioned public Kernel JSON.

use fsl_core::{KERNEL_SCHEMA_ID, KERNEL_SCHEMA_VERSION};
use serde_json::{Map, Value};

pub(crate) fn required_object<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("public Kernel {context} must be an object"))
}

pub(crate) fn required_array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a [Value], String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("public Kernel {context}.{key} must be an array"))
}

pub(crate) fn required_str<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("public Kernel {context}.{key} must be a string"))
}

pub(crate) fn public_kernel_v1_root(kernel: &Value) -> Result<&Map<String, Value>, String> {
    let root = required_object(kernel, "root")?;
    let schema = required_str(root, "$schema", "root")?;
    if schema != KERNEL_SCHEMA_ID {
        return Err(format!(
            "unsupported public Kernel $schema '{schema}'; expected '{KERNEL_SCHEMA_ID}'"
        ));
    }
    let version = required_str(root, "schema_version", "root")?;
    if version != KERNEL_SCHEMA_VERSION {
        return Err(format!(
            "unsupported public Kernel schema_version '{version}'; expected '{KERNEL_SCHEMA_VERSION}'"
        ));
    }
    Ok(root)
}

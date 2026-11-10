// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use sha2::{Digest, Sha256};

// Exactly the nine logical leaves measured in native-run-1, no type-name mask.
pub const TIMINGS: [&str; 9] = [
    "/cost/elapsed_s",
    "/cost/properties/0/elapsed_s",
    "/cost/properties/1/elapsed_s",
    "/cost/properties/2/elapsed_s",
    "/cost/properties/3/elapsed_s",
    "/cost/properties/4/elapsed_s",
    "/cost/properties/5/elapsed_s",
    "/cost/properties/6/elapsed_s",
    "/cost/solver/check_elapsed_s",
];

#[derive(Clone)]
pub struct Packet {
    pub exit: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub artifact: Vec<u8>,
    pub stdout_sha256: String,
    pub artifact_sha256: String,
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Packet {
    pub fn new(exit: i32, stdout: Vec<u8>, stderr: Vec<u8>, artifact: Vec<u8>) -> Self {
        Self {
            exit,
            stdout_sha256: sha256(&stdout),
            artifact_sha256: sha256(&artifact),
            stdout,
            stderr,
            artifact,
        }
    }
}

pub fn artifact_pointer(stdout_pointer: &str) -> String {
    format!("/verification/engine_metadata{stdout_pointer}")
}

fn pretty(value: &Value, stdout: bool) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if stdout {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn checked(packet: &Packet) -> Result<(Value, Value), String> {
    if packet.exit != 1 {
        return Err("exit: expected violating export exit1".to_owned());
    }
    if sha256(&packet.stdout) != packet.stdout_sha256
        || sha256(&packet.artifact) != packet.artifact_sha256
    {
        return Err("raw/hash: inconsistent recorded bytes".to_owned());
    }
    let stdout: Value =
        serde_json::from_slice(&packet.stdout).map_err(|e| format!("stdout JSON: {e}"))?;
    let artifact: Value =
        serde_json::from_slice(&packet.artifact).map_err(|e| format!("artifact JSON: {e}"))?;
    if pretty(&stdout, true)? != packet.stdout || pretty(&artifact, false)? != packet.artifact {
        return Err("framing: stdout pretty+LF / artifact pretty without LF required".to_owned());
    }
    for (document, path) in [
        (&stdout, "/cost/properties"),
        (&artifact, "/verification/engine_metadata/cost/properties"),
    ] {
        let properties = document.pointer(path);
        if properties.and_then(Value::as_array).map(Vec::len) != Some(7) {
            return Err("shape: expected the seven observed property rows".to_owned());
        }
    }
    for pointer in TIMINGS {
        for (document, path) in [
            (&stdout, pointer.to_owned()),
            (&artifact, artifact_pointer(pointer)),
        ] {
            let value = document
                .pointer(&path)
                .ok_or_else(|| format!("missing timing: {path}"))?;
            let duration = value
                .as_f64()
                .ok_or_else(|| format!("numeric timing required: {path}"))?;
            if !duration.is_finite() || duration < 0.0 {
                return Err(format!("finite nonnegative timing required: {path}"));
            }
        }
    }
    let cost = stdout.get("cost").ok_or("missing stdout cost")?;
    let mirror = artifact
        .pointer("/verification/engine_metadata/cost")
        .ok_or("missing artifact cost")?;
    // Serialized complete objects preserve field order as well as all values.
    if pretty(cost, false)? != pretty(mirror, false)? {
        return Err("copy: entire artifact cost differs from stdout cost".to_owned());
    }
    Ok((stdout, artifact))
}

pub fn agree(before: &Packet, after: &Packet) -> Result<(), String> {
    // Both packets are validated first: missing on both sides cannot disappear.
    let (first_stdout, _) = checked(before)?;
    let (mut next_stdout, mut next_artifact) = checked(after)?;
    if before.stderr != after.stderr {
        return Err("stderr: complete bytes differ".to_owned());
    }
    for pointer in TIMINGS {
        let value = first_stdout
            .pointer(pointer)
            .ok_or("validated first timing disappeared")?
            .clone();
        *next_stdout
            .pointer_mut(pointer)
            .ok_or("validated next timing disappeared")? = value.clone();
        *next_artifact
            .pointer_mut(&artifact_pointer(pointer))
            .ok_or("validated mirror disappeared")? = value;
    }
    let derived_stdout = pretty(&next_stdout, true)?;
    let derived_artifact = pretty(&next_artifact, false)?;
    if derived_stdout != before.stdout || derived_artifact != before.artifact {
        return Err("stable: full bytes differ outside the nine validated durations".to_owned());
    }
    if sha256(&derived_stdout) != before.stdout_sha256
        || sha256(&derived_artifact) != before.artifact_sha256
    {
        return Err("derived/hash: scalar-derived raw/hash agreement failed".to_owned());
    }
    // Do not remove cost/counters/memory or rewrite original raw observations.
    Ok(())
}

// SPDX-License-Identifier: Apache-2.0

//! Detector for #1425: the built-in cap must not retain a full spec per
//! discovered mutant. macOS does not enforce `RLIMIT_AS`; run this on Linux.
#![cfg(target_os = "linux")]

use std::fmt::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn capped_mutate_does_not_keep_a_full_spec_for_every_candidate() {
    // Reasoned ceiling, NOT a measured Linux calibration. Each literal const
    // yields two integer +/-1 mutants. With K=8192 the eager implementation
    // retains at least 2*K*K const items. On 64-bit Linux their String headers
    // alone occupy 2*8192*8192*24 = 3 GiB, excluding the rest of SpecItem,
    // boxed expressions, name allocations, and allocator overhead. A 1 GiB
    // virtual-address ceiling therefore rejects eager full-spec retention.
    // Lazy descriptors retain 2*K single edits plus a bounded number of base
    // specs, i.e. linear rather than quadratic storage. This leaves generous
    // headroom for parsing, lowering and Z3 on this one-Bool, depth-1 model.
    // Linux pre-fix red and post-fix green runs are required to calibrate it;
    // macOS compilation alone is not memory-regression evidence.
    const K: usize = 8192;
    const CEILING_KB: u64 = 1024 * 1024;

    let mut source = String::from("spec LazyMutantMemory {\n");
    for index in 0..K {
        writeln!(source, "  const C{index} = 10").expect("append const");
    }
    source.push_str(
        "  state { x: Bool }\n  init { x = false }\n  action keep() { x = x }\n\
         invariant Stable { not x }\n}\n",
    );
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let fixture = Fixture(
        std::env::temp_dir().join(format!("fsl-issue-1425-{}-{nonce}.fsl", std::process::id())),
    );
    std::fs::write(&fixture.0, source).expect("write generated spec");
    let command = format!(
        "ulimit -v {CEILING_KB} && exec {:?} mutate {:?} --depth 1 --max-mutants 1",
        env!("CARGO_BIN_EXE_fslc"),
        fixture.0.to_str().expect("UTF-8 fixture path"),
    );
    let output = Command::new("sh")
        .args(["-c", &command])
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run capped mutate");
    assert!(
        output.status.success(),
        "mutate exceeded {CEILING_KB} KiB or failed: status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("mutate JSON");
    assert_eq!(value["result"], "mutated", "{value:#}");
    assert_eq!(value["baseline"], "verified", "{value:#}");
    assert_eq!(value["summary"]["total"], 1, "{value:#}");
    assert_eq!(value["mutants"].as_array().expect("mutants").len(), 1);
    assert_eq!(value["mutants"][0]["status"], "survived", "{value:#}");
    // Counting only the first candidate (or silently skipping enumeration)
    // must not make this detector green: the complete catalog is reported.
    let dropped = value["notes"]
        .as_array()
        .expect("notes")
        .iter()
        .filter_map(Value::as_str)
        .find_map(|note| {
            note.strip_prefix("mutant cap 1 reached: ")?
                .strip_suffix(" dropped")?
                .parse::<usize>()
                .ok()
        })
        .expect("full catalog cap note");
    assert!(dropped >= 2 * K - 1, "{value:#}");
}

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
    // Each literal produces two +/-1 mutants: eager storage retains at least
    // 2*K*K const items, versus 2*K single edits plus bounded base specs when
    // lazy. Earlier macOS debug K=1024 measurements were ~875 MB eager versus
    // ~64 MB / 1.3 s fixed. Quadratic eager growth predicts ~3.5 GB at K=2048,
    // comfortably above 1 GiB. K=8192 took ~111 s even fixed (mostly baseline
    // const processing); use K=2048 to avoid that unrelated CI-time cost.
    //
    // Direct K=2048 macOS measurements (base 45a2de33, fixed f5bd0e71):
    // eager 3,309,305,856 bytes max RSS / 6.51 s; fixed 72,056,832 / 5.14 s.
    // Commands (FSLC_CACHE=off, with a 60 s timeout on each):
    // /usr/bin/time -l .claude/work/fslc-base-45a2de33 mutate \
    //   .claude/work/generated-1425-final-k2048.fsl --depth 1 --max-mutants 1
    // /usr/bin/time -l rust/target/debug/fslc mutate \
    //   .claude/work/generated-1425-final-k2048.fsl --depth 1 --max-mutants 1
    // Binary SHA256s, respectively:
    // 751d19897ed319c9499aa7f9b5a0a749e9bca8521ee72191b61a5f4dcc6a8b07
    // d21fe2c32b39366afdf7d23045f61af13ae31cfb60e91fc5fdc7c101519e3f35
    //
    // These are macOS RSS measurements, NOT Linux RLIMIT_AS calibration.
    // The 1 GiB virtual-address ceiling has ~15x measured fixed RSS headroom
    // and is ~3x below eager RSS. Linux pre-fix red and fixed green runs are
    // still required: reserved address space can exceed RSS, and allocator /
    // platform differences mean macOS measurements alone are not proof.
    const K: usize = 2048;
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

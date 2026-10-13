// SPDX-License-Identifier: Apache-2.0

//! A refinement whose impl domain differs from the abstraction's used to stop
//! with a runtime error (`result:"error"`, `kind:"type"`, exit 2) instead of a
//! refinement verdict:
//!
//! - an impl parameter type wider than the abstract action's maps a step to an
//!   abstract call whose argument is outside the abstract parameter domain
//!   ("parameter 'v' does not belong to its declared domain for action
//!   'push'");
//! - an impl `Map` key type narrower than the abstraction's leaves the mapped
//!   abstract `Map` without some keys of its finite key domain ("map index
//!   outside finite key domain").
//!
//! Both are deterministic refinement failures and are now reported as
//! `refinement_failed` / `kind:"map_out_of_bounds"` (exit 1) by `fslc refine`
//! and by `fslc verify`'s inline `implements`. In `fslc mutate`, the
//! `type_bound_lo_minus1` mutant that widens the parameter type is killed by
//! `refinement` with `--oracle-attribution` listing `refinement` as a killer
//! (the attribution pass used to drop the error and list no killer).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

const SEQ_ABS: &str = "spec SeqAbs {\n  type V = 0..2\n  state { seq: Seq<V, 2> }\n  \
     init { seq = Seq {} }\n  action push(v: V) {\n    requires seq.size() < 2\n    \
     seq = seq.push(v)\n  }\n}\n";
const SEQ_WIDE_IMPL: &str = "spec SeqWide {\n  type V = -1..2\n  state { seq: Seq<V, 2> }\n  \
     init { seq = Seq {} }\n  action push(v: V) {\n    requires seq.size() < 2\n    \
     seq = seq.push(v)\n  }\n}\n";
const SEQ_MAP: &str = "refinement M {\n  impl SeqWide\n  abs SeqAbs\n  map seq = seq\n  action push(v) -> push(v)\n}\n";

fn seq_req(lo: i32) -> String {
    format!(
        "requirements SeqReq {{\n  implements SeqAbs from \"abs.fsl\" {{ maps auto }}\n\n  \
         type V = {lo}..2\n  state {{ seq: Seq<V, 2> }}\n  init {{ seq = Seq {{}} }}\n  \
         action push(v: V) maps push(v) {{\n    requires seq.size() < 2\n    \
         seq = seq.push(v)\n  }}\n}}\n"
    )
}

const MAP_ABS: &str = "spec MapAbs {\n  type K = 0..2\n  state { m: Map<K, Bool> }\n  \
     init { forall k: K { m[k] = false } }\n  action set(k: K) { m[k] = true }\n  \
     invariant Total { forall k: K { m[k] or not m[k] } }\n}\n";
const MAP_NARROW_REQ: &str = "requirements MapReq {\n  \
     implements MapAbs from \"abs.fsl\" { maps auto }\n\n  type K = 0..1\n  \
     state { m: Map<K, Bool> }\n  init { forall k: K { m[k] = false } }\n  \
     action set(k: K) maps set(k) { m[k] = true }\n}\n";

fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/refine-out-of-domain-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    for (file, content) in files {
        fs::write(dir.join(file), content).expect("write fixture");
    }
    dir
}

fn fslc(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let json = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "fslc {args:?} printed non-JSON ({error}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("exit code"))
}

#[test]
fn refine_reports_an_out_of_domain_abstract_argument_as_map_out_of_bounds() {
    let dir = scratch(
        "refine",
        &[
            ("impl.fsl", SEQ_WIDE_IMPL),
            ("abs.fsl", SEQ_ABS),
            ("map.fsl", SEQ_MAP),
        ],
    );
    let (output, code) = fslc(
        &dir,
        &["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "2"],
    );
    assert_eq!(output["result"], "refinement_failed", "{output}");
    assert_eq!(output["kind"], "map_out_of_bounds", "{output}");
    assert_eq!(output["at"], "step", "{output}");
    assert_eq!(code, 1, "{output}");
}

#[test]
fn inline_implements_reports_domain_mismatches_as_map_out_of_bounds() {
    for (name, req, abs) in [
        ("param", seq_req(-1), SEQ_ABS),
        ("map", MAP_NARROW_REQ.to_owned(), MAP_ABS),
    ] {
        let dir = scratch(name, &[("req.fsl", req.as_str()), ("abs.fsl", abs)]);
        let (output, code) = fslc(&dir, &["verify", "req.fsl", "--depth", "3"]);
        assert_eq!(output["result"], "refinement_failed", "{name}: {output}");
        assert_eq!(
            output["implements"]["violation"]["kind"], "map_out_of_bounds",
            "{name}: {output}"
        );
        assert_eq!(code, 1, "{name}: {output}");
    }
}

#[test]
fn mutate_attributes_the_widened_parameter_mutant_to_refinement() {
    let dir = scratch(
        "mutate",
        &[("req.fsl", seq_req(0).as_str()), ("abs.fsl", SEQ_ABS)],
    );
    let (output, code) = fslc(
        &dir,
        &[
            "mutate",
            "req.fsl",
            "--depth",
            "3",
            "--max-mutants",
            "1",
            "--oracle-attribution",
        ],
    );
    assert_eq!(code, 0, "{output}");
    let mutant = &output["mutants"][0];
    assert_eq!(mutant["op"], "type_bound_lo_minus1", "{output}");
    assert_eq!(mutant["status"], "killed", "{mutant}");
    assert_eq!(mutant["killed_by"], "refinement", "{mutant}");
    assert_eq!(
        mutant["killers"],
        serde_json::json!(["refinement"]),
        "{mutant}"
    );
}

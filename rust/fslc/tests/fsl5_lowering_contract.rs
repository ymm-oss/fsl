// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Lowering contract for the next edition (fsl 5 plan §2, "共通の受け入れ条件").
//!
//! Each example under `tests/fixtures/fsl5/<example>/` holds the design
//! draft's hand-written `lowered.fsl` (the expected lowering, in today's
//! syntax), the `original.fsl` it must stay equivalent to, and the two
//! refinement mappings. `lowered.fsl` is a fixed expectation written by a
//! person: it is never regenerated from a lowering's output, and this test
//! has no golden-update switch. Changing it means reading the diff in review.
//!
//! - **K** (kernel equality): every next-edition source of an example lowers
//!   to exactly the kernel of `lowered.fsl`, compared by
//!   `support/kernel_compare.rs`. The stages that add the syntax add the
//!   sources to [`FSL5_SOURCES`].
//! - **R** (refinement both ways): `lowered.fsl` and the original refine each
//!   other at depth 8.
//! - **M** (the check can fail): one rule of `lowered.fsl` weakened makes R
//!   fail and K differ.
//!
//! Acceptance scenarios do not reach the kernel and are not part of K.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

struct Example {
    name: &'static str,
    /// The abstraction `lowered.fsl` corresponds to name-for-name.
    original: &'static str,
    /// One rule of `lowered.fsl` weakened: `(exact text, replacement)`.
    weakening: (&'static str, &'static str),
    /// What `refine <weakened> <original>` must report.
    weakened_result: &'static str,
}

const EXAMPLES: &[Example] = &[
    Example {
        name: "e2e",
        original: "original.fsl",
        // The decide row `SmallClaim` admits one unit above the limit.
        weakening: (
            "      requires claim_amount[c] <= AUTO_LIMIT\n",
            "      requires claim_amount[c] <= AUTO_LIMIT + 1\n",
        ),
        weakened_result: "refinement_failed",
    },
    Example {
        name: "rejection",
        original: "original.fsl",
        // `allow Operator approves Request only for OperatorHeld` dropped.
        weakening: (
            "fair action approve_tool(r: Request) {\n      requires request_stage[r] == ToolApproval\n      requires request_role[r] == Operator\n",
            "fair action approve_tool(r: Request) {\n      requires request_stage[r] == ToolApproval\n",
        ),
        weakened_result: "refinement_failed",
    },
    Example {
        name: "multi_entity",
        // Entity and stage names of `original.fsl` differ from the lowering;
        // `original_renamed.fsl` is the same model under the lowering's names.
        original: "original_renamed.fsl",
        // `only when ... stock >= qty` dropped: stock can leave its range.
        weakening: ("requires qty > 0 and stock >= qty", "requires qty > 0"),
        weakened_result: "impl_violated",
    },
];

/// `(example, next-edition source)` pairs checked by K. Empty until the
/// stages that add the syntax import the `fsl5_*.fsl` sources;
/// `every_fsl5_source_is_under_k` keeps a source from being added without K.
const FSL5_SOURCES: &[(&str, &str)] = &[];

fn example_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(name)
}

/// `fslc refine <implementation> <abstraction> <mapping> --depth 8` in
/// `dir`, in the published language.
fn refine(dir: &Path, implementation: &str, abstraction: &str, mapping: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "refine",
            implementation,
            abstraction,
            mapping,
            "--depth",
            "8",
        ])
        .current_dir(dir)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .output()
        .expect("run fslc refine");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "refine {implementation} {abstraction} in {}: invalid JSON ({error}): {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

#[test]
fn r_lowered_and_original_refine_each_other() {
    for example in EXAMPLES {
        let dir = example_dir(example.name);
        for (implementation, abstraction, mapping) in [
            (
                "lowered.fsl",
                example.original,
                "refines_lowered_to_original.fsl",
            ),
            (
                example.original,
                "lowered.fsl",
                "refines_original_to_lowered.fsl",
            ),
        ] {
            let (output, exit) = refine(&dir, implementation, abstraction, mapping);
            assert_eq!(
                (output["result"].as_str(), exit),
                (Some("refines"), 0),
                "{}: refine {implementation} {abstraction}: {output}",
                example.name
            );
        }
    }
}

#[test]
fn m_weakened_lowering_fails_r_and_differs_in_k() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_lowering_contract");
    for example in EXAMPLES {
        let source = example_dir(example.name);
        let dir = scratch.join(example.name);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        for entry in std::fs::read_dir(&source).expect("read example") {
            let path = entry.expect("example entry").path();
            std::fs::copy(&path, dir.join(path.file_name().expect("file name")))
                .expect("copy example");
        }
        let lowered = std::fs::read_to_string(source.join("lowered.fsl")).expect("read lowered");
        let (text, replacement) = example.weakening;
        assert_eq!(
            lowered.matches(text).count(),
            1,
            "{}: the weakened rule must occur exactly once in lowered.fsl",
            example.name
        );
        std::fs::write(
            dir.join("weakened.fsl"),
            lowered.replacen(text, replacement, 1),
        )
        .expect("write weakened lowering");

        let (output, exit) = refine(
            &dir,
            "weakened.fsl",
            example.original,
            "refines_lowered_to_original.fsl",
        );
        assert_eq!(
            (output["result"].as_str(), exit),
            (Some(example.weakened_result), 1),
            "{}: refine weakened.fsl {}: {output}",
            example.name,
            example.original
        );

        let difference = kernel_compare::difference(
            &kernel_compare::kernel_v2(&source.join("lowered.fsl")),
            &kernel_compare::kernel_v2(&dir.join("weakened.fsl")),
        );
        assert!(
            difference.is_some(),
            "{}: K must notice the weakened rule",
            example.name
        );
    }
}

#[test]
fn k_fsl5_sources_lower_to_the_lowered_kernel() {
    for &(example, source) in FSL5_SOURCES {
        let dir = example_dir(example);
        kernel_compare::assert_same_kernel(
            &kernel_compare::kernel_v2_fsl5(&dir.join(source)),
            &kernel_compare::kernel_v2(&dir.join("lowered.fsl")),
            &format!("{example}/{source}"),
        );
    }
}

#[test]
fn every_fsl5_source_is_under_k() {
    for example in EXAMPLES {
        for entry in std::fs::read_dir(example_dir(example.name)).expect("read example") {
            let name = entry
                .expect("example entry")
                .file_name()
                .to_string_lossy()
                .into_owned();
            if name.starts_with("fsl5_") {
                assert!(
                    FSL5_SOURCES.contains(&(example.name, name.as_str())),
                    "{}/{name} is a next-edition source without a K check",
                    example.name
                );
            }
        }
    }
}

// SPDX-License-Identifier: Apache-2.0

//! A standalone `cargo test -p fsl-core` invocation observes the distributed feature set.
//! `cargo test --workspace` or an invocation also selecting fslc/fsl-tools tests
//! unifies `float_roundtrip` through their `jsonschema` dev-dependency and passes even on base.
//! CI's `.github/workflows/merge-readiness.yml` core step calls `check_core_contracts`
//! in `tools/check-merge-readiness.sh`, testing only fsl-syntax, fsl-core, fsl-runtime,
//! and fsl-solver; adding `jsonschema` to any of these four crates' dev-dependencies
//! would silence that reduced-graph detector.
//! Guards DESIGN-incremental-verify.md §4's byte-identical cache hits and the
//! `serde_json::from_slice` cache reader in rust/fslc/src/verification.rs.

use serde_json::Value;

fn assert_cache_number_roundtrip(literal: &str, bits: u64) {
    // Do not parse the expected value through the reader under test first.
    let original = Value::from(f64::from_bits(bits));
    let bytes = serde_json::to_vec(&original).expect("serialize original number");
    let replayed: Value = serde_json::from_slice(&bytes).expect("cache-reader parse path");
    let actual = replayed.as_f64().expect("cached f64").to_bits();
    assert_eq!(
        actual,
        bits,
        "cache reader changed {literal}: expected bits {bits:#018x}, actual {actual:#018x}; stored token={}",
        String::from_utf8_lossy(&bytes),
    );
    assert_eq!(
        serde_json::to_vec(&replayed).expect("serialize replayed number"),
        bytes,
        "cache number token changed for {literal}"
    );
}

#[test]
fn cache_reader_preserves_small_property_time_bits_and_token() {
    assert_cache_number_roundtrip("3.3339999999999998e-6", 0x3ecb_f7b5_e22d_39d2);
}

#[test]
fn cache_reader_preserves_solver_time_bits_and_token() {
    assert_cache_number_roundtrip("0.0013088360000000003", 0x3f55_71a7_f436_845e);
}

#[test]
fn cache_reader_preserves_rounded_property_time_bits_and_token() {
    assert_cache_number_roundtrip("2.4160000000000002e-6", 0x3ec4_4452_0bdd_d124);
}

// SPDX-License-Identifier: Apache-2.0

//! #1607: every engine names the first reached concrete binder failure in init.
//! Expectations come from the operations and source statement below, not another
//! engine's output. All fixtures initialize every state variable, so auto uses
//! the concrete path rather than falling back because of nondeterministic init.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn source(expression: &str) -> String {
    format!(
        "spec BinderInit {{\n  type K = 0..2\n  state {{ d: Int, xs: Seq<K, 3>, items: Set<K>, ok: Bool }}\n  init {{\n    d = 0\n    xs = Seq {{ 1 }}\n    items = Set {{ 2, 0 }}\n    ok = {expression}\n  }}\n  action step() {{ }}\n  invariant Expected {{ ok }}\n}}\n"
    )
}

fn verify(expression: &str, engine: &str) -> (Value, i32) {
    // Retain fixtures inside the worktree for fail-first/mutation inspection.
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/issue-1607");
    std::fs::create_dir_all(&directory).expect("fixture directory");
    let path = directory.join(format!(
        "{}-{}.fsl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, source(expression)).expect("write inline fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("verify")
        .arg(&path)
        .args(["--engine", engine, "--depth", "4", "--no-cache"])
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run working-tree CLI");
    assert!(output.stderr.is_empty(), "{engine}: {output:?}");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{engine}: invalid JSON: {error}; {output:?}"));
    let exit = output.status.code().expect("native exit status");
    println!(
        "{engine} expression={expression} fixture={} exit={exit} output={value}",
        path.display()
    );
    (value, exit)
}

fn require_error(expression: &str, engine: &str, stem: &str) {
    let (value, exit) = verify(expression, engine);
    assert_eq!(exit, 2, "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "semantics", "{value:#}");
    // Every expression is the RHS of the assignment at 8:5, even if the
    // failure lies in a collection source or an inner binder's summand.
    assert_eq!(
        value["message"],
        format!("{stem} in init at 8:5"),
        "{value:#}"
    );
    for key in ["loc", "trace"] {
        assert!(value.get(key).is_none(), "{key}: {value:#}");
    }
}

fn require_success(expression: &str, engine: &str) {
    let (value, exit) = verify(expression, engine);
    assert_eq!(exit, 0, "{value:#}");
    assert_eq!(
        value["result"],
        if engine == "bmc" {
            "verified"
        } else {
            "proved"
        },
        "{value:#}"
    );
    for key in ["message", "kind", "loc", "trace"] {
        assert!(value.get(key).is_none(), "{key}: {value:#}");
    }
}

macro_rules! cells {
    ($name:ident, $expression:literal, $assertion:ident $(, $stem:literal)?) => {
        mod $name {
            use super::*;

            #[test]
            fn bmc() {
                $assertion($expression, "bmc" $(, $stem)?);
            }
            #[test]
            fn induction() {
                $assertion($expression, "induction" $(, $stem)?);
            }
            #[test]
            fn explicit() {
                $assertion($expression, "explicit" $(, $stem)?);
            }
            #[test]
            fn auto() {
                $assertion($expression, "auto" $(, $stem)?);
            }
        }
    };
}

// The final mathematical sum fits i64; the second partial sum does not.
cells!(
    sum_prefix_overflow,
    "sum(k in Seq { 9223372036854775807, 1, -1 } of k) == 9223372036854775807",
    require_error,
    "integer overflow in sum"
);
cells!(
    sum_undefined_summand,
    "sum(k in 0..1 of 6 / d) == 0",
    require_error,
    "division by zero"
);
cells!(
    forall_typed_body,
    "forall k: K { 6 % k > 0 }",
    require_error,
    "remainder by zero"
);
cells!(
    exists_set_body,
    "exists k in items { 6 / k > 0 }",
    require_error,
    "division by zero"
);
cells!(
    forall_range_where,
    "forall k in 0..2 where 6 % k > 0 { true }",
    require_error,
    "remainder by zero"
);
cells!(
    exists_sequence_where,
    "exists k in Seq { 1, 0 } where 6 / k > 0 { false }",
    require_error,
    "division by zero"
);
cells!(
    forall_sequence_source,
    "forall k in Seq { xs.at(1) } { true }",
    require_error,
    "at() index out of range"
);
cells!(
    exists_sequence_source,
    "exists k in Seq { xs.pop().head() } { true }",
    require_error,
    "head() on empty sequence"
);
cells!(
    count_set_where,
    "count(k in items where 6 / d > k) == 0",
    require_error,
    "division by zero"
);
// unique must still evaluate the third filter after two matching candidates.
cells!(
    unique_late_where,
    "unique(k in 0..2 where k < 2 or 6 % (k - 2) > 0)",
    require_error,
    "remainder by zero"
);
cells!(
    exactly_one_sequence_source,
    "exactlyOne(k in Seq { xs.at(1) } where k == 0)",
    require_error,
    "at() index out of range"
);
cells!(
    nested_binder_summand,
    "forall i: K { sum(j in 0..1 of 6 / (i + j)) > 0 }",
    require_error,
    "division by zero"
);

// Exercise diagnostic traversal of skipped candidates even when init is
// independently undefined later: the skipped remainder must not obscure that
// division. These cases are intended for short-circuit mutation calibration.
cells!(
    forall_skipped_remainder_before_reached_division,
    "(forall k in 0..1 { k != 0 and 6 % (1 - k) > 0 }) or 6 / d > 0",
    require_error,
    "division by zero"
);
cells!(
    exists_skipped_remainder_before_reached_division,
    "(exists k in 0..1 { k == 0 or 6 % (1 - k) > 0 }) and 6 / d > 0",
    require_error,
    "division by zero"
);

// Keep init independently undefined so diagnostic membership guards, not just
// the separate definedness check, must exclude the skipped remainder failures.
cells!(
    membership_set_absent_candidate_before_reached_division,
    "(forall k in items { 6 % (k - 1) == 0 }) and 6 / d > 0",
    require_error,
    "division by zero"
);
cells!(
    membership_sequence_nonlive_slot_before_reached_division,
    "(forall k in xs { 6 % k == 0 }) and 6 / d > 0",
    require_error,
    "division by zero"
);

cells!(
    controls_forall_short_circuits,
    "not (forall k in 0..1 { k != 0 and 6 / (1 - k) > 0 })",
    require_success
);
cells!(
    controls_exists_short_circuits,
    "exists k in 0..1 { k == 0 or 6 / (1 - k) > 0 }",
    require_success
);
cells!(
    controls_set_filter_skips_body,
    "forall k in items where k != 0 { 6 / k > 0 }",
    require_success
);
cells!(
    controls_sequence_ignores_nonlive_slots,
    "forall k in xs { 6 / k > 0 }",
    require_success
);
cells!(
    controls_defined_aggregate_results,
    "sum(k: K of k) == 3 and count(k in items) == 2 and sum(k in Seq { 1, 1 } of k) == 2 and unique(k in 0..2 where k == 1) and exactlyOne(k in items where k == 2)",
    require_success
);

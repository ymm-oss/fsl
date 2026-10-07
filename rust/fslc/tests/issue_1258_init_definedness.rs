// SPDX-License-Identifier: Apache-2.0

//! Issue #1258: BMC and induction must ask whether `init` is defined.
//!
//! `init` evaluates with the action-context rules (division and remainder by
//! zero, a finite `Map` key outside its domain, checked i64 overflow), and the
//! explicit engine stops with a semantics error when it fails. The symbolic
//! init encoding totalizes those operations, so before the fix `--engine bmc`
//! answered `verified` and `--engine induction` (whose base case is the same
//! BMC) answered `proved` for an init no concrete engine can run. A failing
//! deterministic init also disables the concrete pre-pass, so nothing else
//! caught it; a nondeterministic init never had the pre-pass at all.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-1258-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn text(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn verify(fixture: &Fixture, engine: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.text(),
            "--engine",
            engine,
            "--depth",
            "4",
            "--no-cache",
        ])
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

const SYMBOLIC_ENGINES: [&str; 2] = ["bmc", "induction"];

fn assert_semantics_error(output: &Value, status: i32, message: &str) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    assert_eq!(output["message"], message, "{output:#}");
}

fn assert_clean(output: &Value, status: i32, engine: &str) {
    assert_eq!(status, 0, "{output:#}");
    let expected = if engine == "induction" {
        "proved"
    } else {
        "verified"
    };
    assert_eq!(output["result"], expected, "{output:#}");
}

const INIT_DIV: &str = r"spec InitDiv {
  type Small = 0..3
  state {
    d: Small,
    x: Int
  }
  init {
    d = 0
    x = 6 / d
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

const INIT_MAP_READ: &str = r"spec InitMapRead {
  type K = 0..3
  type I = 0..5
  state {
    m: Map<K, Int>,
    i: I,
    x: Int
  }
  init {
    forall k: K { m[k] = 1 }
    i = 5
    x = m[i]
  }
  action tick() {
    requires i > 0
    i = i - 1
  }
  invariant Ok { x >= 1 }
}
";

/// `InitDiv` plus an unassigned `junk: Bool`: init is nondeterministic, so
/// the concrete pre-pass never runs and the explicit engine refuses it.
const INIT_DIV_ND: &str = r"spec InitDivNd {
  type Small = 0..3
  state {
    d: Small,
    x: Int,
    junk: Bool
  }
  init {
    d = 0
    x = 6 / d
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

const INIT_OVF: &str = r"spec InitOvf {
  type Small = 0..3
  state {
    d: Small,
    x: Int
  }
  init {
    d = 0
    x = 4611686018427387904 * (d + 2)
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

/// A remainder by zero on the reached side of an init `if`.
const INIT_REM_IF_ND: &str = r"spec InitRemIfNd {
  type Small = 0..3
  state {
    d: Small,
    x: Int,
    junk: Bool
  }
  init {
    d = 0
    if d == 0 {
      x = 7 % d
    } else {
      x = 0
    }
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

/// A finite `Map` assignment target outside its key domain.
const INIT_MAP_WRITE_ND: &str = r"spec InitMapWriteNd {
  type K = 0..3
  type I = 0..5
  state {
    m: Map<K, Int>,
    i: I,
    junk: Bool
  }
  init {
    forall j: I { m[j] = 1 }
    i = 5
  }
  action tick() {
    requires i > 0
    i = i - 1
  }
  invariant Ok { i >= 0 }
}
";

#[test]
fn undefined_init_is_a_located_semantics_error_under_bmc_and_induction() {
    let cases = [
        ("div", INIT_DIV, "division by zero in init at 9:5"),
        (
            "map-read",
            INIT_MAP_READ,
            "map index outside finite key domain in init at 12:5",
        ),
        ("div-nd", INIT_DIV_ND, "division by zero in init at 10:5"),
        (
            "ovf",
            INIT_OVF,
            "integer overflow in multiplication in init at 9:5",
        ),
        (
            "rem-if-nd",
            INIT_REM_IF_ND,
            "remainder by zero in init at 11:7",
        ),
        (
            "map-write-nd",
            INIT_MAP_WRITE_ND,
            "map assignment index outside key domain in init at 10:19",
        ),
    ];
    for (name, source, message) in cases {
        let fixture = Fixture::new(name, source);
        for engine in SYMBOLIC_ENGINES {
            let (output, status) = verify(&fixture, engine);
            assert_semantics_error(&output, status, message);
        }
    }
}

/// The explicit engine already stopped on these inits; its envelope is the
/// reference the symbolic messages name.
#[test]
fn explicit_engine_reports_the_same_failures_unchanged() {
    let cases = [
        ("explicit-div", INIT_DIV, "division by zero"),
        (
            "explicit-map-read",
            INIT_MAP_READ,
            "map index outside finite key domain",
        ),
        (
            "explicit-ovf",
            INIT_OVF,
            "integer overflow in multiplication",
        ),
    ];
    for (name, source, message) in cases {
        let fixture = Fixture::new(name, source);
        let (output, status) = verify(&fixture, "explicit");
        assert_semantics_error(&output, status, message);
    }
}

/// Negative controls: a partial operation on the unreached side of an init
/// `if`, conditional expression, or short-circuit stays defined.
const INIT_IF_GUARD: &str = r"spec InitIfGuard {
  type Small = 0..3
  state {
    d: Small,
    x: Int
  }
  init {
    d = 0
    if d > 0 {
      x = 6 / d
    } else {
      x = 0
    }
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

const INIT_CONDITIONAL_ND: &str = r"spec InitConditionalNd {
  type Small = 0..3
  state {
    d: Small,
    x: Int,
    junk: Bool
  }
  init {
    d = 0
    x = if d != 0 then 6 / d else 0
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

const INIT_SHORT_CIRCUIT_ND: &str = r"spec InitShortCircuitNd {
  type Small = 0..3
  state {
    d: Small,
    ok: Bool,
    junk: Bool
  }
  init {
    d = 0
    ok = d == 0 or 6 / d > 1
  }
  action tick() {
    requires d < 3
    d = d + 1
  }
  invariant Ok { ok }
}
";

#[test]
fn reached_side_guards_and_type_bounds_keep_init_defined() {
    let cases = [
        ("if-guard", INIT_IF_GUARD),
        ("conditional-nd", INIT_CONDITIONAL_ND),
        ("short-circuit-nd", INIT_SHORT_CIRCUIT_ND),
    ];
    for (name, source) in cases {
        let fixture = Fixture::new(name, source);
        for engine in SYMBOLIC_ENGINES {
            let (output, status) = verify(&fixture, engine);
            assert_clean(&output, status, engine);
        }
    }
}

/// Negative control: action-context `partial_op` is still the search's own
/// violation (exit 1), not an init error.
const ACTION_DIV: &str = r"spec ActionDiv {
  type Small = 0..3
  state {
    d: Small,
    x: Int
  }
  init {
    d = 0
    x = 1
  }
  action tick() {
    requires d < 3
    x = 6 / d
    d = d + 1
  }
  invariant Ok { x >= 0 }
}
";

#[test]
fn action_partial_operation_stays_a_partial_op_violation() {
    let fixture = Fixture::new("action-div", ACTION_DIV);
    for engine in SYMBOLIC_ENGINES {
        let (output, status) = verify(&fixture, engine);
        assert_eq!(status, 1, "{output:#}");
        assert_eq!(output["result"], "violated", "{output:#}");
        assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
        assert_eq!(output["invariant"], "_partial_tick", "{output:#}");
    }
}

/// Negative control: an unassigned init variable is free, the search reports
/// its type bound at step 0, and the init pass assumes the bounds the search
/// asserts there. A `Map` read keyed by it is therefore not an init failure,
/// and the verdict is the search's own, unchanged.
const INIT_FREE_KEY: &str = r"spec InitFreeKey {
  type K = 0..3
  state {
    m: Map<K, Int>,
    k0: K,
    x: Int
  }
  init {
    forall k: K { m[k] = 1 }
    x = m[k0]
  }
  action tick() {
    requires x > 0
    x = x - 1
  }
  invariant Ok { x >= 0 }
}
";

#[test]
fn a_free_init_key_is_bounded_by_its_type_not_an_init_failure() {
    let fixture = Fixture::new("free-key", INIT_FREE_KEY);
    for engine in SYMBOLIC_ENGINES {
        let (output, status) = verify(&fixture, engine);
        assert_eq!(status, 1, "{output:#}");
        assert_eq!(output["result"], "violated", "{output:#}");
        assert_eq!(output["violation_kind"], "type_bound", "{output:#}");
        assert_eq!(output["invariant"], "_bounds_k0", "{output:#}");
        assert_eq!(output["violated_at_step"], 0, "{output:#}");
    }
}

/// The init pass builds no solver term and asks nothing for an init with no
/// operation that can fail (the shared term table would otherwise move later
/// sessions' models, e.g. the induction CTI), including the total
/// `forall k: K { m[k] = .. }` form. The positive control keeps the absence
/// check from being vacuous.
const INIT_MAP_FILL: &str = r"spec InitMapFill {
  type K = 0..3
  state {
    m: Map<K, Int>,
    n: Int
  }
  init {
    forall k: K { m[k] = 1 }
    n = 0
  }
  action tick() {
    requires n < 3
    n = n + 1
  }
  invariant Ok { n >= 0 }
}
";

fn asks_init_definedness(output: &Value) -> bool {
    output["cost"]["properties"]
        .as_array()
        .expect("cost properties")
        .iter()
        .any(|property| property["kind"] == "init" && property["name"] == "definedness")
}

#[test]
fn an_init_with_nothing_that_can_fail_asks_nothing() {
    for (name, source, asked) in [
        ("fill", INIT_MAP_FILL, false),
        ("action-div-init", ACTION_DIV, false),
        ("if-guard-asked", INIT_IF_GUARD, true),
    ] {
        let fixture = Fixture::new(name, source);
        for engine in SYMBOLIC_ENGINES {
            let (output, _) = verify(&fixture, engine);
            assert_eq!(asks_init_definedness(&output), asked, "{output:#}");
        }
    }
}

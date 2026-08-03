// SPDX-License-Identifier: Apache-2.0

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
            "fsl-issue-651-{name}-{}-{nonce}.fsl",
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

fn verify(fixture: &Fixture) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            fixture.text(),
            "--depth",
            "1",
            "--deadlock",
            "ignore",
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

#[test]
fn bmc_reports_a_replayable_partial_operation_from_nondeterministic_init() {
    let fixture = Fixture::new(
        "nondeterministic-init",
        r"
spec NondeterministicPartial {
  type Small = -3..3
  state { dividend: Small, choose_zero: Bool, quotient: Small }
  init { dividend = -3  quotient = 0 }
  action divide() { quotient = dividend / (if choose_zero then 0 else 1) }
}
",
    );
    let (output, status) = verify(&fixture);

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(output["invariant"], "_partial_divide", "{output:#}");
    assert_eq!(output["violated_at_step"], 1, "{output:#}");
    assert_eq!(output["last_action"]["name"], "divide", "{output:#}");
    assert_eq!(output["trace_type"], "partial_op", "{output:#}");
    assert_eq!(
        output["faithfulness_class"], "partial_op_unguarded",
        "{output:#}"
    );
    assert_eq!(
        output["trace"].as_array().map(Vec::len),
        Some(2),
        "{output:#}"
    );
}

#[test]
fn bmc_preserves_initial_property_precedence_over_future_partial_operations() {
    let fixture = Fixture::new(
        "initial-precedence",
        r"
spec InitialPrecedence {
  type Small = 0..1
  state { divisor: Small, quotient: Small }
  init { divisor = 0  quotient = 0 }
  action divide() { quotient = 1 / divisor }
  invariant InitiallyFalse { quotient == 1 }
}
",
    );
    let (output, status) = verify(&fixture);

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation_kind"], "invariant", "{output:#}");
    assert_eq!(output["invariant"], "InitiallyFalse", "{output:#}");
    assert_eq!(output["violated_at_step"], 0, "{output:#}");
}

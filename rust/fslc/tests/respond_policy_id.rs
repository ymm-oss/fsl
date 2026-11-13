// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1782 / #1276: `preserve progress { respond <ID> by … }` names an abstract
//! leadsTo by its canonical ID. A business policy's leadsTo is named only by
//! its policy ID (`POL-WAIT-001`), so `respond` reads that ID with the same
//! rule as the `policy` declaration, and the refinement checks the property
//! the business spec declares without the spec being rewritten.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

const BUSINESS: &str = "business WaitingBusiness {
  actor System
  entity Ticket
  process Ticket {
    stages Waiting, Held, Done
    initial Waiting
    transition promote Waiting -> Done by System
    transition hold Waiting -> Held by System
    transition release Held -> Done by System
  }
  policy POL-WAIT-001 \"a waiting ticket is promoted\"
    every Ticket in Waiting must eventually be Done or Held
  policy POL-HOLD-002 \"a held ticket is released\"
    every Ticket in Held must eventually be Done
}

verify {
  instances Ticket = 1
}
";

/// Keeps the progress: its only step from `Queued` is the fair `promote`.
const KEEPS: &str = "spec Keeps {
  enum Phase { Queued, Served }
  state { phase: Phase }
  init { phase = Queued }
  fair action promote() { requires phase == Queued   phase = Served }
}
";

/// Loses the progress: the fair `park`/`unpark` loop can spin in `Queued`
/// forever while the unfair `promote` never fires.
const LOSES: &str = "spec Loses {
  enum Phase { Queued, Parked, Served }
  state { phase: Phase }
  init { phase = Queued }
  fair action park() { requires phase == Queued   phase = Parked }
  fair action unpark() { requires phase == Parked   phase = Queued }
  action promote() { requires phase == Queued   phase = Served }
}
";

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "fslc-1782-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create #1782 fixture directory");
        for (file, source) in files {
            std::fs::write(root.join(file), source).expect("write #1782 fixture");
        }
        Self(root)
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(arguments)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run native fslc")
    }

    fn json(&self, arguments: &[&str]) -> (Value, i32) {
        let output = self.run(arguments);
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON: {error}; args={arguments:?}; stdout={}; stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().expect("native exit status"))
    }
}

fn mapping(implementation: &str, respond: &str) -> String {
    let stutters = if implementation == "Loses" {
        "  action park() -> stutter\n  action unpark() -> stutter\n"
    } else {
        ""
    };
    format!(
        "refinement R {{
  impl {implementation}
  abs WaitingBusiness
  map ticket_stage[t: Ticket] = if phase == Served then Done else Waiting
{stutters}  action promote() -> promote(0)
  preserve progress {{
    respond {respond} by promote
  }}
}}
"
    )
}

fn refine(implementation: &str, respond: &str) -> (Value, i32) {
    let file = if implementation == "Keeps" {
        ("impl.fsl", KEEPS)
    } else {
        ("impl.fsl", LOSES)
    };
    let mapping = mapping(implementation, respond);
    Fixture::new(&[file, ("abs.fsl", BUSINESS), ("map.fsl", &mapping)])
        .json(&["refine", "impl.fsl", "abs.fsl", "map.fsl"])
}

#[test]
fn a_flow_that_keeps_a_policy_id_progress_refines() {
    for _ in 0..2 {
        let (envelope, exit) = refine("Keeps", "POL-WAIT-001");
        assert_eq!(exit, 0, "{envelope:#}");
        assert_eq!(envelope["result"], "refines", "{envelope:#}");
    }
}

#[test]
fn a_flow_that_loses_a_policy_id_progress_is_progress_lost() {
    for _ in 0..2 {
        let (envelope, exit) = refine("Loses", "POL-WAIT-001");
        assert_eq!(exit, 1, "{envelope:#}");
        assert_eq!(envelope["result"], "refinement_failed", "{envelope:#}");
        assert_eq!(envelope["kind"], "progress_lost", "{envelope:#}");
        assert_eq!(envelope["invariant"], "POL-WAIT-001", "{envelope:#}");
        assert_eq!(
            envelope["progress"]["leadsTo"], "POL-WAIT-001",
            "{envelope:#}"
        );
    }
}

#[test]
fn a_respelled_policy_id_is_unknown_and_the_diagnostic_lists_the_declared_names() {
    for respond in ["POL_WAIT_001", "POL-WAIT-1", "pol-wait-001", "POLWAIT001"] {
        let (envelope, exit) = refine("Keeps", respond);
        assert_eq!(exit, 2, "{respond}: {envelope:#}");
        assert_eq!(envelope["result"], "error", "{respond}: {envelope:#}");
        assert_eq!(envelope["kind"], "type", "{respond}: {envelope:#}");
        assert_eq!(
            envelope["message"],
            format!(
                "unknown abstract leadsTo '{respond}'; abstract spec 'WaitingBusiness' \
                 declares leadsTo 'POL-WAIT-001', 'POL-HOLD-002'"
            ),
            "{envelope:#}"
        );
        assert_eq!(envelope["loc"]["line"], 7, "{envelope:#}");
    }
}

#[test]
fn an_abstraction_without_leads_to_says_so() {
    let abstraction = "spec Abs {
  enum Stage { Open, Closed }
  state { stage: Stage }
  init { stage = Open }
  action close() { requires stage == Open   stage = Closed }
}
";
    let mapping = "refinement R {
  impl Keeps
  abs Abs
  map stage = if phase == Served then Closed else Open
  action promote() -> close()
  preserve progress {
    respond POL-WAIT-001 by promote
  }
}
";
    let (envelope, exit) = Fixture::new(&[
        ("impl.fsl", KEEPS),
        ("abs.fsl", abstraction),
        ("map.fsl", mapping),
    ])
    .json(&["refine", "impl.fsl", "abs.fsl", "map.fsl"]);
    assert_eq!(exit, 2, "{envelope:#}");
    assert_eq!(envelope["kind"], "type", "{envelope:#}");
    assert_eq!(
        envelope["message"],
        "unknown abstract leadsTo 'POL-WAIT-001'; abstract spec 'Abs' declares no leadsTo",
        "{envelope:#}"
    );
}

/// The quoted spelling is not a second way to write a name: a policy is not
/// declared with a quoted ID either.
#[test]
fn a_quoted_name_is_refused() {
    let (envelope, exit) = refine("Keeps", "\"POL-WAIT-001\"");
    assert_eq!(exit, 2, "{envelope:#}");
    assert_eq!(envelope["result"], "error", "{envelope:#}");
    assert_eq!(
        envelope["message"], "expected requirement identifier",
        "{envelope:#}"
    );
    assert_eq!(envelope["loc"]["line"], 7, "{envelope:#}");
    assert_eq!(envelope["loc"]["column"], 13, "{envelope:#}");
}

#[test]
fn fmt_prints_a_respond_policy_id_as_written() {
    let mapping = mapping("Keeps", "POL-WAIT-001");
    let next = format!("fsl 5\n{mapping}");
    let fixture = Fixture::new(&[
        ("map.fsl", &mapping),
        ("map5.fsl", &next),
        (
            "spaced.fsl",
            &mapping.replace("POL-WAIT-001", "POL - WAIT -  001"),
        ),
    ]);
    for (file, source) in [("map.fsl", mapping.as_str()), ("map5.fsl", next.as_str())] {
        let output = fixture.run(&["fmt", file]);
        assert_eq!(output.status.code(), Some(0), "{file}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), source, "{file}");
        let (check, exit) = fixture.json(&["fmt", "--check", file]);
        assert_eq!(exit, 0, "{check:#}");
        assert_eq!(check["changed"], false, "{check:#}");
    }
    let output = fixture.run(&["fmt", "spaced.fsl"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), mapping);
}

/// The inline seam still refuses `preserve progress` (#1275) at the
/// `preserve` keyword, before the name is read.
#[test]
fn inline_implements_still_refuses_preserve_progress_with_a_policy_id() {
    let requirements = "requirements Inline {
  enum Phase { Queued, Served }
  state { phase: Phase }
  init { phase = Queued }
  requirement REQ-1 \"promote\" {
    fair action promote() { requires phase == Queued   phase = Served }
  }
  implements WaitingBusiness from \"abs.fsl\" {
    map ticket_stage[t: Ticket] = if phase == Served then Done else Waiting
    action promote() -> promote(0)
    preserve progress {
      respond POL-WAIT-001 by promote
    }
  }
}
";
    let (envelope, exit) = Fixture::new(&[("abs.fsl", BUSINESS), ("req.fsl", requirements)])
        .json(&["check", "req.fsl"]);
    assert_eq!(exit, 2, "{envelope:#}");
    assert_eq!(envelope["kind"], "parse", "{envelope:#}");
    assert_eq!(envelope["loc"]["line"], 11, "{envelope:#}");
    assert_eq!(envelope["loc"]["column"], 5, "{envelope:#}");
    let message = envelope["message"].as_str().expect("error message");
    assert!(
        message.contains("preserve progress is not valid inside implements"),
        "{envelope:#}"
    );
}

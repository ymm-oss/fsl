// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `violation_kind` axis: the 12-value `fsl_verifier::violation_kind::ALL`
//! vocabulary (issue #646, #537 C3 slice 1). Rows are *referenced* from
//! `fsl_verifier::violation_kind::ALL`, not re-owned.
//!
//! Declared columns: `BMC`, `induction` -- the two fsl-verifier engines that
//! can populate a `BmcViolation`/`InductionCti`/`RankFailure.kind` field (or,
//! for `deadlock`, the BMC-only deadlock probe). Every cell is filled:
//! `leadsTo`/`deadlock` are BMC-only, `invariant`/`trans`/the 8 ranked-leadsTo
//! kinds are induction-only, so exactly half of this 12x2 matrix is
//! `NotApplicable` by construction -- each with a citation to where that
//! structural fact is codified (see this module's `SLICE1_BOUNDARY` and
//! `INDUCTION_ONLY_RANKING` bases, and `docs/DESIGN-assurance-matrix.md`'s
//! "Slice 1 boundary" section, which records the reasoned scope decision
//! this axis's N/A cells rest on).

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_verifier::violation_kind as vk;

use crate::claim::{Axis, Citation, Claim};

/// Basis for every `NotApplicable` cell whose structural fact is "this
/// engine never emits this kind because of a Slice 1 file-scope boundary
/// documented in the design doc" (the plain-BMC `invariant`/`trans`
/// literals, and the bounded `leadsTo` kind's absence from the induction
/// engine).
const SLICE1_BOUNDARY: Citation = Citation {
    path: "docs/DESIGN-assurance-matrix.md",
    anchor: "### Slice 1 boundary",
};

/// Basis for every `NotApplicable` cell whose structural fact is "the
/// induction engine is the sole caller of `prove_ranked_leadstos`, and the
/// bounded BMC engine never attempts a ranking proof" -- cites the CLI's own
/// engine dispatch, which routes `--engine induction` exclusively through
/// `run_induction_filtered`/`prove_induction`/`prove_ranked_leadstos`.
const INDUCTION_ONLY_RANKING: Citation = Citation {
    path: "rust/fslc/src/verification.rs",
    anchor: "Ok(VerificationEngine::Induction) => run_induction_filtered(InductionRequest {",
};

/// Basis for the `deadlock` row's `induction` N/A cell: the induction engine
/// dispatch never calls `verify_bounded` (the sole caller of the BMC
/// deadlock probe), so it structurally cannot emit `deadlock`. Same citation
/// as [`INDUCTION_ONLY_RANKING`] -- the dispatch table is the single fact
/// both N/A reasons rest on.
const INDUCTION_NEVER_CALLS_BMC: Citation = INDUCTION_ONLY_RANKING;

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn axis() -> Axis {
    let rows = vk::ALL.to_vec();
    let columns = vec!["BMC", "induction"];
    let mut cells: BTreeMap<(&'static str, &'static str), Claim> = BTreeMap::new();

    cells.insert(
        (vk::LEADS_TO, "BMC"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/issue_260_leadsto_stagnation.rs",
                anchor: "fn leadsto_deadlock_stagnation_is_detected_beyond_the_deadlock_step()",
            },
        },
    );
    cells.insert(
        (vk::LEADS_TO, "induction"),
        Claim::NotApplicable {
            reason: "the induction engine's leadsTo obligations surface through the 8 ranked-proof kinds (unbounded_below, ...), never the bounded `leadsTo` kind, which only the BMC engine's bounded search (bmc.rs::leadsto_violation) emits",
            basis: SLICE1_BOUNDARY,
        },
    );

    cells.insert(
        (vk::INVARIANT, "induction"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/induction_suggestions.rs",
                anchor: "fn suggests_a_scalar_bound_without_changing_the_verdict()",
            },
        },
    );
    cells.insert(
        (vk::INVARIANT, "BMC"),
        Claim::NotApplicable {
            reason: "the plain (non-induction) BMC engine's invariant violations are emitted by a separate, out-of-Slice-1-scope site (bmc.rs's make_violation, which mirrors fsl_runtime::Monitor's own registered outcome.kind spelling and is that registry's concern, not this one's)",
            basis: SLICE1_BOUNDARY,
        },
    );

    cells.insert(
        (vk::TRANS, "induction"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/induction_suggestions.rs",
                anchor: "fn trans_ctis_never_receive_invariant_suggestions()",
            },
        },
    );
    cells.insert(
        (vk::TRANS, "BMC"),
        Claim::NotApplicable {
            reason: "same boundary as `invariant`: the plain BMC engine's transition violations are a separate, out-of-Slice-1-scope emission site",
            basis: SLICE1_BOUNDARY,
        },
    );

    let ranked = [
        (
            vk::UNBOUNDED_BELOW,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_rank_kind_vocabulary.rs",
                anchor: "fn unbounded_measure_is_reported_as_unbounded_below()",
            },
        ),
        (
            vk::PROGRESS_ACTION_NOT_FAIR,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_helpful_ranking.rs",
                anchor: "fn helpful_does_not_create_fairness()",
            },
        ),
        (
            vk::HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_helpful_ranking.rs",
                anchor: "fn multiple_helpful_actions_with_flickering_enabledness_is_not_falsely_proved()",
            },
        ),
        (
            vk::HELPFUL_ACTION_NOT_ENABLED,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_helpful_ranking.rs",
                anchor: "fn blocked_helpful_action_is_reported()",
            },
        ),
        (
            vk::NON_DECREASING_ACTION,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_rank_kind_vocabulary.rs",
                anchor: "fn a_non_helpful_stall_with_no_helpful_declared_is_non_decreasing_action()",
            },
        ),
        (
            vk::PENDING_NOT_PRESERVED,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_rank_kind_vocabulary.rs",
                anchor: "fn a_non_helpful_action_leaving_both_p_and_q_false_is_pending_not_preserved()",
            },
        ),
        (
            vk::NON_DECREASING_HELPFUL_ACTION,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_rank_kind_vocabulary.rs",
                anchor: "fn a_helpful_action_that_stalls_the_measure_is_non_decreasing_helpful_action()",
            },
        ),
        (
            vk::NON_HELPFUL_ACTION_INCREASES_MEASURE,
            Citation {
                path: "rust/fsl-verifier/tests/leadsto_helpful_ranking.rs",
                anchor: "fn non_helpful_action_pumping_the_measure_is_not_falsely_proved()",
            },
        ),
    ];
    for (kind, citation) in ranked {
        cells.insert((kind, "induction"), Claim::Exercised { by: citation });
        cells.insert(
            (kind, "BMC"),
            Claim::NotApplicable {
                reason: "ranked/inductive leadsTo liveness proof (prove_ranked_leadstos) is induction-engine-only; the bounded BMC engine never attempts a ranking proof",
                basis: INDUCTION_ONLY_RANKING,
            },
        );
    }

    cells.insert(
        (vk::DEADLOCK, "BMC"),
        Claim::Exercised {
            by: Citation {
                path: "rust/fslc/tests/issue_260_leadsto_stagnation.rs",
                anchor: "fn deadlock_error_still_wins_over_leadsto_beyond_the_deadlock_step()",
            },
        },
    );
    cells.insert(
        (vk::DEADLOCK, "induction"),
        Claim::NotApplicable {
            reason: "the induction engine's CLI dispatch never calls verify_bounded (the sole caller of the BMC deadlock probe); --engine induction routes exclusively through prove_induction/prove_ranked_leadstos, neither of which examines enabled-action counts",
            basis: INDUCTION_NEVER_CALLS_BMC,
        },
    );

    Axis {
        name: "violation_kind",
        rows,
        columns,
        cells,
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn model(source: &str) -> fsl_core::KernelModel {
    let kernel =
        parse_kernel_source(source, &FsResolver::new(std::env::temp_dir())).expect("parse");
    build_model(kernel).expect("build model")
}

const LEADS_TO_SRC: &str = r"
spec AssuranceProbeLeadsTo {
  state { done: Bool }
  init { done = false }
  action idle() {
    done = false
  }
  leadsTo Finishes {
    done == false ~> done == true
  }
}
";

const DEADLOCK_SRC: &str = r"
spec AssuranceProbeDeadlock {
  type Level = 0..3
  state { level: Level }
  init { level = 0 }
  action inc() {
    requires level < 3
    level = level + 1
  }
}
";

const INVARIANT_CTI_SRC: &str = r"
spec AssuranceProbeInvariantCti {
  type Level = 0..5
  state { level: Level }
  init { level = 0 }
  action inc() {
    requires level < 5
    level = level + 1
  }
  invariant NeverAtMax { level < 5 }
}
";

const TRANS_CTI_SRC: &str = r"
spec AssuranceProbeTransCti {
  state { x: Int }
  init { x = 0 }
  action inc() {
    x = x + 1
  }
  action dec() {
    x = x - 1
  }
  trans NeverDecrease { x >= old(x) }
}
";

const UNBOUNDED_BELOW_SRC: &str = r"
spec AssuranceProbeUnboundedBelow {
  state { x: Int, done: Bool }
  init { x = 0  done = false }
  action step() {
    requires done == false
    x = x - 1
  }
  action finish() {
    requires x < -3
    done = true
  }
  leadsTo Finishes {
    done == false ~> done == true
    decreases x
  }
}
";

const PROGRESS_ACTION_NOT_FAIR_SRC: &str = r"
spec AssuranceProbeNonfairHelpful {
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }
  action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  action idle(c: Case) {
    level[c] = level[c]
  }
  invariant NonNeg { forall c: Case { level[c] >= 0 } }
  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

const HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY_SRC: &str = r"
spec AssuranceProbeFlickeringHelpful {
  type Phase = 0..9
  type Credit = 0..1
  state { phase: Phase, done: Bool, credit: Credit }
  init { phase = 0  done = false  credit = 1 }
  fair action helpEven() {
    requires phase % 2 == 0
    requires done == false
    done = true
    credit = 0
  }
  fair action helpOdd() {
    requires phase % 2 == 1
    requires done == false
    done = true
    credit = 0
  }
  action rotate() {
    requires done == false
    phase = (phase + 1) % 10
  }
  leadsTo Finishes {
    done == false ~> done == true
    helpful helpEven()
    helpful helpOdd()
    decreases credit
  }
}
";

const HELPFUL_ACTION_NOT_ENABLED_SRC: &str = r"
spec AssuranceProbeBlockedHelpful {
  type Case = 0..1
  type Level = 0..2
  state {
    level: Map<Case, Level>,
    gate: Map<Case, Bool>
  }
  init {
    forall c: Case {
      level[c] = 2
      gate[c] = false
    }
  }
  fair action step(c: Case) {
    requires gate[c]
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  invariant NonNeg { forall c: Case { level[c] >= 0 } }
  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

const NON_DECREASING_SRC: &str = r"
spec AssuranceProbeNonDecreasing {
  type Level = 0..5
  state { level: Level, done: Bool }
  init { level = 5  done = false }
  action wiggle() {
    requires level > 0
    level = level - 1
  }
  action stall() {
    requires done == false
    level = level
  }
  leadsTo Finishes {
    done == false ~> done == true
    decreases level
  }
}
";

const PENDING_NOT_PRESERVED_SRC: &str = r"
spec AssuranceProbePendingNotPreserved {
  type Phase = 0..5
  state { phase: Phase }
  init { phase = 0 }
  fair action advance() {
    requires phase == 0
    phase = 2
  }
  action derail() {
    requires phase == 0
    phase = 5
  }
  leadsTo Finishes {
    phase == 0 ~> phase == 2
    helpful advance()
    decreases (2 - phase)
  }
}
";

const NON_DECREASING_HELPFUL_SRC: &str = r"
spec AssuranceProbeNonDecreasingHelpful {
  type Credit = 0..5
  state { progressed: Bool, credit: Credit }
  init { progressed = false  credit = 3 }
  fair action stallHelpful() {
    requires progressed == false
    credit = credit
  }
  leadsTo Finishes {
    progressed == false ~> progressed == true
    helpful stallHelpful()
    decreases credit
  }
}
";

const NON_HELPFUL_ACTION_INCREASES_MEASURE_SRC: &str = r"
spec AssuranceProbePumpedMeasure {
  state { x: Int }
  init { x = 5 }
  fair action work() {
    requires x > 0
    x = x - 1
  }
  action pump() {
    requires x > 0
    x = x + 2
  }
  invariant NonNeg { x >= 0 }
  leadsTo Finishes {
    x > 0 ~> x == 0
    helpful work()
    decreases x
  }
}
";

/// Independent bidirectional check (issue #646, mirroring
/// `conformance_coverage.rs`'s `every_outcome_kind_the_corpus_emits_is_registered_and_exercised`):
/// runs one small, self-contained spec per registered `violation_kind` value
/// directly through `fsl_verifier`'s public API (not through any other
/// test's fixtures, so this check is independent of them), collects every
/// `kind` actually produced, and asserts the observed set equals
/// `violation_kind::ALL` exactly -- catching both a kind that stops firing
/// (dead registry entry) and a kind that fires but was never registered
/// (the #646 defect class).
#[test]
fn every_violation_kind_the_probe_corpus_emits_is_exactly_the_registered_set() {
    let mut observed = BTreeSet::new();

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let leadsto = block_on(fsl_verifier::verify_bounded(
        &model(LEADS_TO_SRC),
        &mut solver,
        3,
    ))
    .expect("bmc leadsTo probe");
    observed.insert(
        leadsto
            .leadsto_violation
            .expect("expected a leadsTo violation")
            .kind,
    );

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let deadlock = block_on(fsl_verifier::verify_bounded(
        &model(DEADLOCK_SRC),
        &mut solver,
        4,
    ))
    .expect("bmc deadlock probe");
    assert!(
        deadlock.deadlock_step.is_some(),
        "expected a deadlock within depth 4"
    );
    observed.insert(vk::DEADLOCK.to_owned());

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let invariant = block_on(fsl_verifier::prove_induction(
        &model(INVARIANT_CTI_SRC),
        &mut solver,
        1,
    ))
    .expect("induction invariant probe");
    observed.insert(invariant.cti.expect("expected an invariant CTI").kind);

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let trans = block_on(fsl_verifier::prove_induction(
        &model(TRANS_CTI_SRC),
        &mut solver,
        1,
    ))
    .expect("induction trans probe");
    observed.insert(trans.cti.expect("expected a trans CTI").kind);

    for source in [
        UNBOUNDED_BELOW_SRC,
        PROGRESS_ACTION_NOT_FAIR_SRC,
        HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY_SRC,
        HELPFUL_ACTION_NOT_ENABLED_SRC,
        NON_DECREASING_SRC,
        PENDING_NOT_PRESERVED_SRC,
        NON_DECREASING_HELPFUL_SRC,
        NON_HELPFUL_ACTION_INCREASES_MEASURE_SRC,
    ] {
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
        let ranked = block_on(fsl_verifier::prove_ranked_leadstos(
            &model(source),
            &mut solver,
        ))
        .expect("prove_ranked_leadstos probe");
        observed.insert(ranked.failure.expect("expected a rank failure").kind);
    }

    let registered = vk::ALL
        .iter()
        .map(|&kind| kind.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed, registered,
        "the probe corpus's observed violation_kind values must equal fsl_verifier::violation_kind::ALL exactly \
         (add a matching const/probe for a value only on one side, per issue #646)"
    );
}

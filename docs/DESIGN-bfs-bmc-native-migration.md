<!-- SPDX-License-Identifier: Apache-2.0 -->

# Native BFS/BMC parity matrix for issue #761

Status: Accepted. Acceptance baseline: `df96d094a658eb7e91c24a8ef1beb494e9b31e5c`
(`origin/main` on 2026-08-26).

## 1. Decision

Retire the frozen-Python BFS/BMC differential only after its still-live detectors are
owned by one native, API-level matrix. The matrix belongs under
`rust/fslc/tests/typed_agreement/`; it calls `fsl_runtime::Monitor`,
`fsl_runtime::bfs`, `fsl_runtime::verify_explicit`,
`fsl_verifier::verify_bounded`, and the existing replay/agreement APIs directly. It
does not spawn Python or the temporary `fsl-bmc` / `fsl-replay-actions` binaries.

The migration has four rules:

1. Preserve the current 20-model breadth with a checked native inventory; do not
   inherit `kernel_cases()` or `can_monitor()` from Python.
2. Compare semantic projections, not presentation envelopes or solver-selected
   witness bytes. A witness is valid when the other lineage accepts the same steps
   and terminal classification.
3. Make earliest reportable deadlock agree with the language definition: a state
   satisfying `terminal` is not a deadlock. This resolves, rather than preserves,
   the current conflict between the language/design contract and legacy BFS.
4. Do not compare `states_explored` across engines. It is an algorithmic/cost
   observation, not an FSL-language result. Keep its focused memory-regression owner.

This is cross-engine agreement with two semantic lineages, not a vote and not a new
public assurance class. The symbolic lineage is BMC/native Z3. `Monitor`, legacy
BFS, and explicit BFS share the concrete evaluator and are one declared concrete
lineage, not three independent observers.

## 2. Authority and confirmed current state

The language contract excludes intended terminal states from deadlock checking
(`docs/LANGUAGE.md:44-48,748-756`). BMC implements that exclusion before recording
`deadlock_step` (`rust/fsl-verifier/src/bmc.rs:370-425`), and explicit BFS does the
same (`rust/fsl-runtime/src/explicit.rs:189-207,268-283`). Legacy
`fsl_runtime::bfs` currently records every state with no enabled action as a
deadlock, without checking `terminal` (`rust/fsl-runtime/src/lib.rs:2021-2028`).

That implementation difference is reflected in two conflicting native records:

- `rust/fslc/tests/typed_agreement/inventory.v1.json:54-58` excludes `deadlock_step` as
  intentionally non-identical.
- the accepted current-architecture record requires Monitor, legacy BFS, and
  symbolic verification to agree on earliest deadlock before the Python harnesses
  are retired (`docs/DESIGN-rust-component-internals.md:587-600`).

The language and accepted migration precondition win. The inventory exclusion must
self-retire when legacy BFS becomes terminal-aware. Merely renaming the legacy value
to “quiescence” would preserve a field whose public name and Python detector both say
deadlock, and would leave the accepted three-way anchor unsatisfied.

Existing native ownership was checked at the cited assertions, not inferred from
test names:

- normalized `(kind, name, step)` verdicts are compared at
  `rust/fslc/tests/typed_agreement/engines.rs:346-350,363-437`;
- clean-run reachability and action coverage are compared at
  `rust/fslc/tests/typed_agreement/engines.rs:439-485`;
- explicit/BMC violation traces are compared and both replayed at
  `rust/fslc/tests/typed_agreement/engines.rs:496-526`;
- generated depth/closure metadata is checked at
  `rust/fslc/tests/typed_agreement/engines.rs:351-357,368-423`;
- the broad native corpus test compares explicit/BMC result, exit, and violation
  depth, but not reachability/deadlock/coverage, at
  `rust/fslc/tests/explicit_engine.rs:291-355`;
- the focused reachable owner compares only the shortest witness step at
  `rust/fslc/tests/explicit_engine.rs:482-495`;
- P2 compares symbolic and explicit violation kind, name, step, complete trace, and
  source span, and corrupts state/step/kind/name/action/location independently
  (`rust/fslc/tests/triangulated/p2_witness_replay.rs:162-181,184-233`);
- the production BMC output gate replays violation, leadsTo, reachable, and deadlock
  traces (`rust/fslc/src/verification_output.rs:737-771`), but its current rejecting witness
  mutation is a violation-state mutation (`rust/fslc/tests/solver_fail_closed.rs:451-474`), not a
  reachable/deadlock-specific calibration.

The acceptance baseline also includes the broader #761 native-owner work that
landed after the analysis baseline. F1 corpus result/exit conservation and F2
dialect induction are native-owned (`rust/fslc/tests/corpus_check_sweep.rs:245-365`;
`rust/fslc/tests/dialect_induction_contract.rs:80-145`). F3's sixteen induction
envelopes and exits are native-owned
(`rust/fslc/tests/induction_cli_contract.rs:423-512`), F4's three-case liveness
lasso replay and nine isolated corruptions landed in #903
(`rust/fslc/tests/liveness_witness_replay.rs:124-147,167-242`), and F6's complete
stable refinement projection and live exclusion control are native-owned
(`rust/fslc/tests/refine_corpus_parity.rs:116-175,887-985`). These completed
owners do not change the BFS/BMC matrix decision. The terminal/deadlock defect
on which its first implementation PR depends is filed as
[#904](https://github.com/ymm-oss/fsl/issues/904).

## 3. What the Python harnesses actually detect

### 3.1 Model roster

Both BFS and BMC harnesses call `kernel_cases(root)` before filtering to direct
children of `specs/`, then exclude `can_monitor == false`
(`tools/check_rust_bfs_parity.py:90-100`;
`tools/check_rust_bmc_parity.py:178-193`). At this
baseline, filtering the surface candidates before kernel export yields exactly 20
supported models and no `can_monitor` exclusions:

| Family | Current paths |
|---|---|
| Direct specs (18) | `audit_log`, `auth_lockout`, `bank`, `bank_impl`, `cart_buggy`, `cart_fixed`, `cart_impl`, `cart_v1`, `cart_v1_buggy`, `inventory_reservation`, `job_pipeline`, `mutex_queue`, `order_workflow`, `payment`, `rate_limiter`, `repair_loop`, `seat_booking`, `seat_booking_impl` |
| Compose (2) | `bank_system`, `order_system` |
| Explicit non-model exclusions (3) | `bank_refines.fsl`, `cart_refines.fsl`, `seat_refines.fsl` |

The current Python selector is itself no longer a reliable inventory API:
`kernel_cases()` exports every surface `spec`/`compose` before the `specs/` filter,
and presently stops on
`examples/gallery/errors/semantics_compose_component_parse_failure.fsl`. This is the
same stable stale-membership precondition recorded by the PR #896 review. Native
migration must therefore encode the 20 paths directly and check the roster in both
directions; it must not transliterate the failing helper order.

`mutex_queue.fsl` is the only listed model with `leadsTo`. Legacy BFS does not
evaluate that property and explicit verification rejects the whole model
(`rust/fslc/tests/typed_agreement/engines.rs:313-323,376-393`). Its matrix posture is therefore
explicit: compare the safety/reachability/deadlock/coverage projection on
Monitor↔legacy-BFS↔BMC, assert the explicit rejection reason contains `leadsTo`, and
leave liveness verdict/replay to the dedicated liveness owners. It must not be
silently skipped.

### 3.2 Complete detector ledger

“Owned” below means the current native assertion detects the same field-level drift.
“Residual” means related code exists but the exact old detector, model breadth, or
direction is not yet calibrated. `P` is frozen Python, `M` direct native Monitor
enumeration, `LB` legacy `fsl_runtime::bfs`, `E` native explicit BFS, and `S` native
symbolic BMC.

| ID | Compared field / condition | Python harness models and direction | Confirmed native owner | Disposition |
|---|---|---|---|---|
| D01 | roster membership and unsupported accounting | 20 selected `specs/` models; selector→case loop | Broad `E↔S` corpus scan exists (`rust/fslc/tests/explicit_engine.rs:291-355`) but is not the same checked roster | **Residual:** native bidirectional inventory |
| D02 | `spec` identity | all cases; `P↔LB`, and `P↔LB` + `P↔S` in BMC | Result structs carry `spec`, but cited agreement assertions do not compare it | **Residual:** assert every observation equals `model.name` |
| D03 | `states_explored` | all BFS cases; exact `P↔LB` (`tools/check_rust_bfs_parity.py:52-63,78-87,111-119`) | One focused value is pinned by `rust/fsl-runtime/tests/issue_730_bfs_memory_ceiling.rs:60-89`; symbolic BMC has no analogue | **Residual retired:** do not migrate engine equality |
| D04 | violation presence | all cases; exact optional projection on `P↔LB` and `P↔S` | `Verdict` equality (`rust/fslc/tests/typed_agreement/engines.rs:94-108,346-350,426-437`) | Owned |
| D05 | violation kind | violating cases, same directions | Same `Verdict`; P2 also compares kind (`rust/fslc/tests/triangulated/p2_witness_replay.rs:171-179`) | Owned |
| D06 | violation name | violating cases, same directions | Same `Verdict`; P2 also compares name | Owned |
| D07 | earliest violation step | violating cases, same directions | Same `Verdict`; broad corpus also compares depth (`rust/fslc/tests/explicit_engine.rs:336-340`) | Owned |
| D08 | reached-property key set / presence | all BFS cases; clean-only in BMC, `P↔LB` and `P↔S` | Full maps are compared on clean generated runs (`rust/fslc/tests/typed_agreement/engines.rs:439-471`) | Owned primitive; apply checked roster |
| D09 | earliest reachable step per key | same as D08 | Same assertion; focused shortest-step control (`rust/fslc/tests/explicit_engine.rs:482-495`) | Owned primitive; apply checked roster |
| D10 | earliest reportable `deadlock_step` | all BFS cases; clean-only in BMC, `P↔LB` and `P↔S` | Explicit and BMC each implement terminal-aware detection, but no cited cross-engine assertion; inventory excludes it | **Residual:** resolve terminal conflict and compare |
| D11 | action-coverage key set | all BFS cases; clean-only in BMC, `P↔LB` and `P↔S` | Full maps compared on clean generated runs (`rust/fslc/tests/typed_agreement/engines.rs:472-485`) | Owned primitive; apply checked roster |
| D12 | action covered/uncovered Boolean | same as D11 | Same assertion | Owned primitive; apply checked roster |
| D13 | violation witness: step/state/action/params/changes and identity, `S→concrete` | every emitted BMC violation; Rust BMC→Rust Monitor and Rust BMC→Python Monitor (`tools/check_rust_bmc_parity.py:83-119,201-204`) | P2 exact identity/replay and isolated corruptions; production replay gate | Owned |
| D14 | reachable witness semantic replay, `S→concrete` | every emitted BMC reachable witness (`tools/check_rust_bmc_parity.py:88-90,95-119`) | Generic production replay exists, but no reachable-specific rejecting corruption over the 20-model matrix | **Residual:** matrix application + calibration |
| D15 | deadlock witness semantic replay, `S→concrete` | every emitted BMC deadlock witness (`tools/check_rust_bmc_parity.py:92-119`) | Generic production replay exists, but no deadlock-specific rejecting corruption | **Residual:** matrix application + calibration |
| D16 | violation witness, `concrete→S` | Python BMC→Rust Monitor in the old direction; native analogue is explicit/Monitor→symbolic | P2 exact `E↔S` trace/identity and typed-agreement exact violation trace | Owned |
| D17 | reachable witness, `concrete→S` | Python BMC→Rust Monitor for every reachable trace (`tools/check_rust_bmc_parity.py:121-175`) | Only shortest step is compared; the concrete trace is not checked symbolically | **Residual:** validate every edge and final reachable predicate |
| D18 | deadlock witness, `concrete→S` | Python BMC→Rust Monitor for every deadlock trace | No native reverse semantic witness check | **Residual:** validate every edge and final non-terminal deadlock |

Aggregate: **18 detector classes: 10 exactly owned, 8 not exactly owned**. Seven
residuals move to native tests (D01, D02, D10, D14, D15, D17, D18); D03 is explicitly
retired rather than converted into a language contract.

The old BMC decision projection deliberately omits `states_explored`, and includes
reachability/deadlock/coverage only when no violation exists
(`tools/check_rust_bmc_parity.py:59-76,194-220`). The native matrix keeps that
stop-order caveat: compare auxiliary maps across all engines only for clean runs,
and use focused single-purpose cases for each non-clean branch. It must not compare
partially accumulated post-violation observations.

## 4. Native test design

### 4.1 Files and responsibilities

Extend the existing integration-test crate rather than introduce another semantic
owner:

```text
rust/fslc/tests/typed_agreement.rs
rust/fslc/tests/typed_agreement/
  engines.rs              # existing normalized observations and common comparator
  corpus_matrix.rs        # checked 20-path inventory and field/edge matrix
  witness_matrix.rs       # S→M replay and E/M→S semantic witness validation
  inventory.v1.json       # add matrix edges and self-retire deadlock exclusion
```

`corpus_matrix.rs` owns a source-coupled constant table with path, depth, expected
axes, and engine posture. A completeness test scans only repository-root
`specs/*.fsl`, classifies the three `*_refines.fsl` files explicitly as mappings,
and requires exact set equality with the 20 model rows. Every matrix row must parse
and build through native `parse_kernel_source` + `build_model`; a failed build is a
test failure, not a skip. A new model or mapping is therefore an explicit inventory
change.

The breadth tier runs all 20 models at depth 2, preserving the old harness default
without making the expensive corpus the sole detector. Existing small fixtures make
each branch live cheaply:

| Axis | Focused case and bound | Required observation |
|---|---|---|
| clean cyclic | `specs/bank.fsl`, depth 2 | no violation/deadlock, reachable and coverage live |
| non-initial violation | `specs/cart_buggy.fsl`, depth 4 | invariant violation at the bound |
| reachable witness | `explicit_reachable_witnessed.fsl`, depth 2 | `HitTwo` witness |
| unintended deadlock | `explicit_deadlock.fsl`, depth 1 | earliest reportable deadlock + trace |
| intended terminal | `assurance_terminal_once.fsl`, depth 1 | no reportable deadlock |
| terminal negative sibling | `assurance_terminal_once_missing.fsl`, depth 1 | the same stop is a deadlock |
| uncovered action | `scenario_blocked_action.fsl`, depth 1 | covered/uncovered Boolean split |

The focused rows are controls, not replacements for the 20-model breadth. Their
expected values are declared before execution so a matrix with no violation or no
deadlock cannot pass vacuously.

### 4.2 Normalized observations and edges

Extend the test-only direct-Monitor enumeration already started by
`observe_monitor` (`rust/fslc/tests/typed_agreement/engines.rs:226-293`). Its observation records:

- model name and requested depth;
- normalized first violation `(kind, name, step)`;
- every reachable name with `Option<earliest_step>`;
- terminal-aware earliest deadlock and its trace;
- a complete action-name→covered map;
- parent links needed only when a witness is found.

Do not add `states_explored` to the comparable projection. It may be printed as
debug evidence, but the comparator type must make it impossible to equate that
field accidentally.

Required edges are:

1. `M↔LB`: exact name, safety verdict, reachability, earliest terminal-aware
   deadlock, and coverage for every matrix case.
2. `LB↔E`: the same projection for the 19 non-`leadsTo` cases.
3. `E↔S`: the same projection for those 19 cases; auxiliary maps only on clean
   runs.
4. `LB↔S`: the safety projection for `mutex_queue.fsl`, while an assertion proves
   why `E` is absent.
5. `S-witness→M`: call the existing `replay_bmc_witnesses` / `replay_trace`
   path for every violation, reachable, and deadlock witness.
6. `E/M-witness→S`: for every successful edge call
   `transition_matches_step`; for a violating final outcome use
   `transition_outcome_matches_step`; then independently check the final reachable
   predicate or reportable-deadlock predicate in the symbolic lineage.

The reverse deadlock edge needs one small backend-neutral agreement helper in
`rust/fsl-verifier/src/agreement.rs`, analogous to `expression_matches_value`: pin a
complete concrete state, prove every action instance disabled, evaluate terminal
with definedness, and return true only for `disabled && !terminal`. It must not call
the concrete `terminal_holds` helper. The reachable reverse edge can use the
existing symbolic expression-value agreement API. This preserves two reviewably
different decision lineages.

Witness trace bytes are never compared between independently selected witnesses.
`docs/LANGUAGE.md:1098-1102` explicitly permits non-unique BMC witnesses; exact
equality is valid only where P2 deliberately uses the same fixed model to bind
identity.

### 4.3 Earliest-deadlock repair

Move explicit BFS's terminal evaluation into a private shared runtime helper and
use it from both explicit and legacy BFS. Legacy BFS then records
`deadlock_step = min(step)` only when `enabled.is_empty() && !terminal_holds`.
Undefined/partial terminal evaluation must follow the existing explicit/BMC
fail-closed behavior, including `_partial_property_terminal`, rather than being
folded into either deadlock or terminal.

After the positive and negative terminal siblings pass, remove
`excluded_observation_fields.deadlock_step` from
`rust/fslc/tests/typed_agreement/inventory.v1.json` and add
`earliest_deadlock` to the required edges. A source-coupled inventory test must fail
if the exclusion and required edge are both absent or both present.

### 4.4 Rejecting-control calibration

Each label below is earned by executing an isolated known-bad observation. The test
must assert the produced `AgreementFailure.field` and produced value beside the
expected value.

| Detector | Isolated mutation | Expected cut |
|---|---|---|
| roster | remove one known model row; separately add an unregistered synthetic path | exact-set check reports missing / extra path |
| spec identity | replace one observation's model name | `field=spec` |
| violation | toggle presence; then mutate kind, name, and step one at a time | the corresponding verdict field |
| reachability | remove a reached key; change its earliest step | `reachable_presence`, then `reachable_step` |
| deadlock | shift `deadlock_step`; classify the terminal fixture as deadlocked | `deadlock_step`, then `terminal_deadlock` |
| coverage | delete one action key; flip one known covered Boolean | `coverage_key`, then `coverage_value` |
| generic trace integrity | mutate step, state, action, params, and changes separately | concrete replay rejects the named field |
| reachable `S→M` | corrupt an intermediate state; separately keep a valid trace but relabel its final state as satisfying the target | replay, then reachable classification rejects |
| deadlock `S→M` | append an enabled action or replace the final state with a live one | replay/deadlock classification rejects |
| reachable `M/E→S` | change one successor while retaining action/params; separately use a final state where the target is false | symbolic transition, then symbolic target check rejects |
| deadlock `M/E→S` | change the final state to one with an enabled action; separately use an intended terminal | symbolic disabledness, then terminal exclusion rejects |

P2's current state/step/kind/name/action/location mutations remain and are not
relabelled as reachable/deadlock detectors. New tests add params and changes because
`replay_trace` promises both (`rust/fsl-runtime/src/lib.rs:2749-2873`). Every new or
changed control runs twice in one session before being reported stable.

No mutation asserts cross-engine `states_explored`. The existing Linux memory
ceiling and its fixed reproducer count remain a performance-preservation control,
not evidence of general semantic equality.

## 5. Binary and Python-harness retirement order

`fsl-bfs` remains. `issue_730_bfs_memory_ceiling.rs` launches it under `RLIMIT_AS`
and asserts the reproducer's state count
(`rust/fsl-runtime/tests/issue_730_bfs_memory_ceiling.rs:60-89`),
so replacing that process boundary is not part of this migration.

The safe removal sequence is:

1. Land the terminal-aware legacy BFS and native corpus/witness matrix with all
   rejecting controls.
2. Delete `tools/check_rust_bfs_parity.py` and `tools/check_rust_bmc_parity.py`;
   delete `rust/fslc/src/bin/fsl-bmc.rs`; update every disposition/reference.
3. In the same cleanup train, delete `tools/check_rust_scenarios_parity.py` only after its
   accepted downstream condition is met: native scenario identity tests remain,
   BMC/cover/deadlock witnesses are replay-gated, and the new matrix owns the shared
   witness semantics. This script imports `DEFAULT_REPLAY_BIN` and
   `_json_normalize` from the BMC harness
   (`tools/check_rust_scenarios_parity.py:19-30`), so deleting
   BMC first without deleting or detaching scenarios leaves a broken import.
4. F4's native lasso replay matrix has landed in #903, including all three legacy
   cases and isolated state/action/loop corruptions
   (`rust/fslc/tests/liveness_witness_replay.rs:124-147,167-242`). Do **not** delete
   `fsl-replay-actions` merely because BFS/BMC/scenarios are gone: the retained
   Python consumer still imports its path and executes it
   (`tools/check_rust_leadsto_parity.py:22,70-100,148-155`). Remove the binary in
   the cleanup that deletes that consumer, or after deliberately migrating the
   retained consumer to a production replay surface. This is a hard dependency,
   not optional cleanup.

Before deleting either binary, use repository-wide reference checks, then `cargo
build --workspace --locked` to prove Cargo's auto-discovered bin targets and tests no
longer require them. Update at least `docs/RUST-PORTING.md`,
`docs/DESIGN-rust-integration.md`, `docs/DESIGN-ci.md`,
`docs/DESIGN-rust-component-internals.md`, `rust/README.md`, the
`tools/check_rust_full_envelope.py` witness-ownership comment, and a `changelog.d/`
fragment. Do not edit `CHANGELOG.md` directly.

## 6. Triangulated-assurance posture

The matrix satisfies the AGENTS invariant that symbolic verification, concrete
Monitor behavior, and solver-free BFS agree, while keeping runtime independent of
solver crates: all symbolic calls live in the `fslc-rust` integration-test target;
`fsl-runtime` gains no solver dependency.

No new `TriangulatedClaim` is required if the result is described accurately as
native cross-engine agreement. P2 remains the registered triangulated claim for the
fixed invariant-witness identity scope
(`rust/fslc/tests/triangulated/p2_witness_replay.rs:29-100`). The broad matrix may
cite P2 but may not claim that Monitor, legacy BFS, and explicit BFS are independent
observers; they share the concrete evaluator.

If a later PR labels the broad matrix “triangulated” or expands P2's registered
scope, it must also expand the claim contract in
`docs/DESIGN-triangulated-assurance.md:25-49,51-79`: preserve the raw trace before
classification, declare symbolic and concrete lineages, execute model↔world,
oracle↔world, and model↔oracle edges, cite accepting and rejecting controls, and
state the exact manifest revision/depth/backend/platform scope. The matrix never
promotes `bounded` or changes a process exit
(`docs/DESIGN-triangulated-assurance.md:21-23`).

## 7. PR split and verification

Use four dependent PRs. “About 300 lines” applies to added/changed implementation
and test logic; mechanical deletion of the old harness bodies is counted and
reviewed separately.

| PR | Scope | Approximate logical delta | Focused verification |
|---|---|---:|---|
| 1. Terminal-aware legacy BFS | shared terminal helper; legacy BFS behavior; terminal positive/rejecting tests; inventory exclusion self-retirement | ≤250 lines | `cargo test -p fsl-runtime --locked`; focused typed-agreement deadlock test twice; `cargo fmt --all -- --check`; targeted Clippy |
| 2. Checked corpus decision matrix | `corpus_matrix.rs`, exact 20+3 inventory, D01/D02/D04-D12 edges and comparator corruptions | ≤300 lines | roster test twice; corpus matrix twice; existing `typed_agreement` and `explicit_engine`; targeted Clippy |
| 3. Bidirectional witness matrix | `witness_matrix.rs`, symbolic final-state/deadlock helper, D14-D18 controls; retain P2 scope | ≤300 lines | witness matrix twice; `transition_agreement`; `triangulated_assurance`; focused P2 fault operators |
| 4. Retirement and docs | delete BFS/BMC/scenarios harnesses and `fsl-bmc`; delete the now-native-owned F4 Python consumer with `fsl-replay-actions`; update docs/changelog | additions ≤200; deletions larger | no dangling references; `cargo test --workspace --locked`; `cargo build --workspace --locked`; `./tools/check-native-integration.sh` |

PR 1 must land first. PR 2 and PR 3 may be developed after PR 1, but both touch the
typed-agreement support surface and should merge serially. PR 4 is last and is
blocked on PRs 1-3. The F4 native-owner condition for `fsl-replay-actions` was
satisfied by #903; its retained Python consumer and the binary still move together.

Respect the two-cargo-track limit:

- track A is solver-free (`fsl-runtime` and its tests);
- track B is solver-backed (`fsl-verifier` / `fslc-rust`, native Z3).

At most one command per track may run concurrently, and shared target-directory
locking may require serial execution. Never launch a third cargo process to make up
for a slow Z3 build. The full workspace and product gate run once, serially, after
the focused checks. PR 4 is not complete until the full native product evidence is
green; merge readiness alone is insufficient.

## 8. Exit criteria and risks

Migration is complete only when:

- the 20-model inventory and three mapping exclusions agree exactly with `specs/`;
- every required edge runs at the same declared bound and all focused branches are
  witnessed;
- terminal and non-terminal stopping siblings distinguish reportable deadlock;
- every BMC violation/reachable/deadlock witness replays concretely, and every
  concrete witness is admitted and classified symbolically;
- every named rejecting mutation fails for the expected field, twice;
- no Python process or helper binary is used by the new matrix;
- no cross-engine `states_explored` assertion has been introduced;
- the retained F4 consumer is reconciled before `fsl-replay-actions` deletion.

The primary risk is accidentally canonizing legacy “no enabled action, including an
intended terminal” as deadlock just to preserve old parity. That would contradict the
language contract and could make all engines agree on the wrong semantics. The
terminal sibling pair and removal of the stale inventory exclusion are therefore the
first PR, not cleanup deferred until after the broad matrix.

The stale Python selector and remaining migration controls are dispositioned by
existing issue [#761](https://github.com/ymm-oss/fsl/issues/761). The terminal
mismatch is separately filed as [#904](https://github.com/ymm-oss/fsl/issues/904)
because it is an existing language-contract defect and the first migration
prerequisite, not merely matrix implementation work.

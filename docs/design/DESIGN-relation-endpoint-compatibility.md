<!-- SPDX-License-Identifier: Apache-2.0 -->

# Relation endpoint carrier compatibility (#1522, related #1524)

## Decision

Use a located check-time refusal, not symbolic key-union grids. Relation values
are compatible only when both corresponding finite endpoint carriers agree.
`types_compatible` uses the shared `same_carrier` helper to resolve each numeric
alias to its lower and upper bounds and compare those carriers without enumerating
their values. Different aliases with equal bounds remain compatible. The
[whole-container decision](DESIGN-finite-container-carriers.md) (#1472, #1627)
extends this helper to Set, Seq and Map carriers, including Map keys and Seq
capacity. Scalar Int/Range compatibility, nominal enums and
`reachable`/`acyclic`'s identical-declared-endpoint rule are unchanged.

The shared compatibility rule reaches conditional arms, Set/Seq items and
nested expression positions, state/init assignments, and public Kernel expected
types. Declared relation-valued struct fields and Map values already fail the
state-type boundary; this does not admit them. An unannotated `let x = q` does not
mix two types and remains valid; a mixed conditional/literal in a let or an
incompatible subsequent use is checked by the same rule. Use existing authored
locations and assignability errors (`kind: semantics`, exit 2), without new spans
or CLI exceptions.
This includes action assignment `r = q`, dependency-ordered init
`q = Set {} r = q`, `let x = q` followed by `r = x`, and a mixed conditional in
a let. Sibling probes confirm these refusals through check and Kernel v1/v2;
the plain alias `let x = q` without an incompatible use remains accepted.

The refusal includes **narrow-to-wide relation copies** in action assignments,
init and let uses. On base, the action-assignment and let reproductions passed
check, but BMC/induction refused with `cannot coerce symbolic value`, while
explicit decided correctly and Monitor replay agreed with that verdict. For
init, BMC/induction and explicit all exited 2 while Monitor replay evaluated the
form correctly. This engine disagreement justifies a uniform check-time refusal
rather than treating widening as supported. #1472 applies the same refusal to
numeric `Set<A>` → `Set<B>` assignment when the carriers differ, even with A's
carrier inside B's, resolving #1627. Equal-bound aliases remain compatible.

## Breaking removal and explicit remainder

Previously accepted mixes such as `relation A -> B` and `relation B -> B`, with
`A = 0..1` and `B = 0..2`, are removed even if a particular runtime value is
empty. Source-only, target-only and equal-size shifted carriers are all covered;
carrier size alone is not compatibility. Both branch orders and nested items
are rejected by check, every verify engine, and replay before evaluation.

This closes #1522's refusal alternative and references, rather than closes,
#1524. **With `N = 0..2`, `M = 0..2`, `r: relation N -> N`, `q: relation M -> M`,
`Seq { r, q }.at(1)` still passes check but exits 2 in the verification engines.**
The equivalent Set/nested literal support is also unfinished. That admitted
composite-literal semantics stays with #1467; this change neither implements it
nor pins engine refusal as acceptable semantics. Equal-bound conditionals
continue to agree with Monitor, including a missing-pair rejecting control.

Out-of-carrier numeric arguments to an accepted relation `add` retain #1541's
actual-pair semantics. This change compares relation *types*, not individual
scalar arguments, and must not replace retained values with silent drops.

## Acceptance and type-boundary evidence

`rust/fslc/tests/issue_1522_relation_endpoint_bounds.rs` v2 pins relation
refusals and preservation controls, including action/init/let widening and a
Set-widening control, subsequently migrated to a refusal by #1472. Conditional
errors retain arm location 11:60;
literal/nested errors retain the existing enclosing authored location 11:3.
No verdict, message, location or exit difference is allowlisted.

The command shapes `<base-binary> check <path>` and `<head-binary> check <path>`
were run twice over all 603 `.fsl` files under `specs/`, `examples/`, and
`rust/fslc/tests/fixtures/`, plus all 23 materialized v2 acceptance forms and the
rewritten `OPUS_REACHABLE` fixture from
`rust/fslc/tests/issue_1459_review_round1.rs`. Base
`299ac3df220d8734be5dca15def4b84c9375264e` and built implementation
`00ee199e2573568fc68ca0fb70e0c9f58806fc19` each used its own
materialized source tree and binary; source blobs were checked against the
named commit. The one rewritten/renamed #1459 test is included as required by
the type-boundary rule; no other existing test was rewritten or stopped.
Existing tests remain required by the workspace gate.

Both runs measured **627 forms = 603 corpus + 24 inline forms**, with
**10 ok→error, 1 diagnostic-only error→error, 0 error→ok and 0 STOP conditions**.
The corpus ok→error set is **measured empty across all 603 forms** using those
commands in the respective
materialized trees of both SHAs; this is not an assumption that all corpus
inputs were initially accepted.
The ten removals are exclusively acceptance forms with different endpoint
bounds; each changed from check exit 0 to located semantics/assignability exit 2:

| Removal ID | Carrier difference | Authored location |
|---|---|---|
| `if-reversed` | source `0..1` versus `0..2`, reversed arms | 11:60 |
| `if-shifted` | source `1..2` versus `0..1`, equal size but different bounds | 11:60 |
| `if-source` | source `0..2` versus `0..1` | 11:60 |
| `if-target` | target `0..3` versus `0..2` | 11:60 |
| `nested` | source `0..2` versus `0..1` in nested Seq/Set items | 11:3 |
| `seq-at` | source `0..2` versus `0..1` in Seq items | 11:3 |
| `set` | source `0..2` versus `0..1` in Set items | 11:3 |
| `act_pos` | target `0..2` → `0..4` in narrow-to-wide action assignment | 6:51 |
| `init_pos` | target `0..2` → `0..4` in narrow-to-wide init assignment | 5:21 |
| `let_pos` | target `0..2` → `0..4` in narrow-to-wide assignment through a let | 6:55 |

Diagnostic-only change: #1459's `OPUS_REACHABLE` changes the self-relation message at 10:3 to the unassignable-relation-type message at 10:51; both remain semantics errors, exit 2.

All 603 corpus check output envelopes (including versions), stderr and exits
agree between base and head: **zero full-output differences**, not just zero
removals. Complete observations also repeat identically across the two runs;
no repeated-output field is excluded. The 24 fresh inline input
paths and their command arguments were separately live-checked as run metadata.
Error→error classification compares complete outputs, stderr and exits except
versions when present on both sides; both absent means no exclusion, and
asymmetric presence is a STOP. The OPUS change affects only message/location
under that comparison. Any removal outside differing endpoint bounds remains
a stop condition rather than an exclusion.

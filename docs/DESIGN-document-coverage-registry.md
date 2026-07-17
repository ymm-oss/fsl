<!-- SPDX-License-Identifier: Apache-2.0 -->

# RCIR coverage registry and no-silent-omission gate

Status: accepted. Implements issue #328.

This design makes the RCIR v1 projector's (issue #325) core discipline — every
authored semantic target is classified into exactly one of `rendered` /
`unattributed` / `unsupported`, never silently dropped — into an explicit,
statically checkable fact rather than an implicit property of the projector's own
code. It adds no new projection behavior: every target the projector already
classifies is classified exactly as before. `--strict` erroring on an unattributed
or unsupported authored target (`FSL-DOC-UNTAGGED-TARGET` / `FSL-DOC-UNSUPPORTED-
TARGET`) was already implemented in issue #327's `document_diagnostics_error` and
needed no change here.

## What was already true, and what was missing

`document_project.rs` already fails closed at *projection time*: after classifying
every authored target, it recomputes the union of `rendered`/`unattributed`/
`unsupported` and returns an error if that union doesn't exactly equal the
authored-target universe, or if any target landed in more than one bucket. That
invariant is real and already tested (issue #325's `rust/fsl-tools/tests/
document.rs`), but it only protects targets the projector's own code already knows
to push into `universe` in the first place. If a new authored-element *kind* were
added to `KernelModel` (a new top-level collection, the same way `actions`/
`invariants`/`transitions`/... already exist) and the projector were never updated
to loop over it, nothing would push it into `universe` at all — the runtime
invariant would stay satisfied, and the omission would be completely silent. This
issue's coverage registry and its coupled-change tests close exactly that gap,
mirroring the existing Public Kernel conformance coverage matrix's discipline
(`rust/fslc/src/coverage.rs`, `rust/fslc/tests/conformance_coverage.rs`,
`docs/DESIGN-kernel-contract.md`) at the scope this issue actually needs — RCIR's
classification is a plain two-way rendered/unsupported split, not the Kernel
matrix's three-level declared/exercised/missing evidence system, so this registry
does not replicate that system's full machinery.

## The registry

`rust/fsl-tools/src/document_coverage.rs` adds `RCIR_TARGET_KIND_REGISTRY`: a fixed
table of every semantic-target *kind* (not individual target — one row per kind,
regardless of how many targets of that kind a given spec authors) the projector
recognizes, each tagged `Rendered` or `Unsupported` with a short description:

| Kind | Treatment | Kernel-native? |
|---|---|---|
| `action` | Rendered (`operation`) | yes |
| `property:invariant` | Rendered (`state_rule`/`deadline_rule`) | yes |
| `property:trans` | Rendered (`transition_rule`) | yes |
| `property:reachable` | Rendered (`reachability_goal`) | yes |
| `property:leadsTo` | Rendered (`progress_rule`) | yes |
| `terminal` | Rendered (`terminal_rule`) | yes |
| `init` | Unsupported | yes |
| `acceptance` | Rendered (`acceptance_trace`) | no (requirements-surface only) |
| `forbidden` | Rendered (`forbidden_trace`) | no (requirements-surface only) |
| `projection` | Unsupported (KPI) | no (requirements-surface only) |
| `refinement` | Unsupported (`implements`) | no (requirements-surface only) |

`target_kind(target: &str) -> String` extracts a raw target string's kind prefix
(`"action:submit_cancel"` -> `"action"`, `"property:invariant:NoReopen"` ->
`"property:invariant"`, `"acceptance:AC-1"` -> `"acceptance"`); this is the join key
between a projected `RequirementClaimSet::coverage`'s target strings and the
registry.

Seven of the eleven kinds come directly from a `KernelModel` collection field
(`actions`, `invariants`, `transitions`, `reachables`, `leadstos`, `terminal`,
`init`) and are therefore part of the dialect-neutral Public Kernel contract. The
other four — `acceptance`/`forbidden` (from `RequirementsTraceContract`, itself
derived from the surface `requirements` AST, not `KernelModel`), `projection` (KPI,
a `requirements`-dialect business-reporting construct), and `refinement`
(`implements`, also surface-level) — are requirements-dialect-only concepts RCIR
also projects, but they do not appear in `kernel.v1.schema.json` at all. This split
matters for how the two coupled-change tests below are built.

## Two coupled-change tests, not one

`rust/fsl-tools/tests/document_coverage.rs` mirrors the Kernel coverage matrix's own
two-test pattern (`semantics_schema_keys_are_all_registered_as_feature_rows` +
`every_outcome_kind_the_corpus_emits_is_registered_and_exercised`), because each
half catches a distinct failure mode neither alone would:

1. **`kernel_native_target_kinds_match_the_public_kernel_v1_schema_required_keys`**
   — bidirectional, schema-anchored. Cross-references the registry's seven
   Kernel-native rows against `kernel.v1.schema.json`'s own `required` (top-level
   `init`/`actions`) and `properties.properties.required`
   (`invariants`/`transitions`/`reachables`/`leads_to`/`terminal`) key lists, via
   `assert_eq!` on the two sets. This is the test that actually closes the gap
   the runtime invariant cannot: if a wholly new Kernel-level semantic element is
   added to the language and (per the Kernel schema's own coupled-change
   discipline) to this required-key list, this test fails until the RCIR
   projector is updated to classify it — independent of whether any fixture
   happens to exercise it yet.
2. **`every_authored_target_kind_across_the_corpus_is_registered_and_every_
   registered_kind_is_exercised`** — bidirectional, corpus-observed. Re-projects
   the existing fixture corpus (`examples/pm/cancel_system.fsl`, `document_claims_
   fixture.fsl`, `document_kpi_fixture.fsl` — the same three files issues
   #325-#327 already established) and compares the *union* of every distinct
   `target_kind()` its combined `coverage.authored` produces against the full
   registry, via `assert_eq!`. This test's other direction — a registered kind
   with no exercising fixture — is not vacuous here: the three existing fixtures
   between them already exercise every one of the four requirements-surface-only
   kinds the schema-anchored test cannot see (`cancel_system.fsl`'s own
   `implements CancelFlow from "cancel_flow.fsl"` exercises `refinement`, its
   `acceptance`/`forbidden` blocks exercise those two kinds, and `document_kpi_
   fixture.fsl`'s `kpi paid_claims = ...` exercises `projection`), so this test is
   fully bidirectional today, not merely a one-way corpus-observed check.
3. **`every_fixture_authored_target_is_exactly_partitioned`** — the issue's
   literal "projection completeness" deliverable: for each corpus fixture,
   `rendered + unattributed + unsupported == authored` (both as counts and as a
   disjoint-union-of-sets check over the actual target strings, which is strictly
   stronger than a count equality — three buckets whose *sizes* sum correctly
   could still disagree on *which* targets they contain). This is a confirming,
   explicit regression test over an invariant the projector already enforces at
   runtime; its value is making the guarantee visible and independently checked,
   not introducing a new one.

Both coupled-change tests are deliberately implemented as ordinary `assert_eq!`
over `BTreeSet`s rather than a shared harness — the registry is small (11 rows)
and the comparison is a plain set equality, so a bespoke harness (as the Kernel's
`Level`/`Evidence` system provides for its own, considerably larger and
multi-dimensional matrix) would be more machinery than this issue's scope needs.

## Verification evidence

`rust/fsl-tools/tests/document_coverage.rs` (3 tests, described above). Manually
verified the no-silent-omission gate actually bites: removing the `terminal` row
from `RCIR_TARGET_KIND_REGISTRY` fails both coupled-change tests with a message
naming exactly the missing kind, restoring it passes again — confirming the gate
would catch a real future omission, not just tautologically agree with itself.

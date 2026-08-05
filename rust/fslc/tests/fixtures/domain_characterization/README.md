<!-- SPDX-License-Identifier: Apache-2.0 -->

# Domain expression characterization corpus

This corpus freezes the pre-typed-expression domain frontend. It is evidence for
the later migration, not a new language contract. Update its baselines only when
an intentional semantic or diagnostic change has been accepted and documented.

- `expressions_valid.fsl` covers canonical logical operators and legacy `->`, bare
  enum members, finite membership, `can(Command)`, aggregate state references,
  scalar/field assignments, defaults, invariants, and stale-policy expressions.
- `lvalues_surface.fsl` covers root, index, and field lvalue parsing, including a
  `Map<K, V>` domain state field with no explicit default (issue #691: fixed --
  `Context::default_for_type` in `rust/fsl-core/src/domain.rs` is now total over
  the field's `SyntaxTypeExprKind`, and the top-level state-field loop in
  `domain_kernel_source` renders the same dense per-key `forall` init
  `domain_lowering.rs`'s path A already generated).
- `container_defaults_surface.fsl` covers `Option<T>`/`Set<T>` domain state
  fields with no explicit default (issue #691's other two affected variants;
  registered in `rust/fsl-core/tests/domain_render_agreement.rs`'s
  `VALID_DOMAIN_FIXTURES` so the two lowering paths' agreement on this shape
  stays gated, not just this corpus's own characterization).
- `effect_saga_valid.fsl` covers expressions used by effect and saga lowering.
- `invalid_empty_enum_containers.fsl` is the rejecting control that keeps empty
  enum validation ahead of both typed lowering and rendered-kernel generation,
  including direct, `Option`, `Set`, Map-key, and Map-value positions.
- `invalid_duplicate_enum.fsl` keeps the repeated member's original location
  and `name` diagnostic classification across both the `domain expand`
  renderer boundary and the `domain check` load path.
- `on_stale` is captured in the surface projection for parser/AST fidelity, but
  since #711 it is rejected fail-closed by `validate_lowerable_constructs`
  before either lowering path runs (`domain_stale_policy_rejected.fsl` in
  `rust/fsl-core/tests/domain_render_agreement.rs`'s
  `SEMANTICALLY_INVALID_DOMAIN_FIXTURES`): no accepted design pins `on_stale`
  semantics, so this is now an accepted, intentional rejection rather than a
  recorded omission. `expressions_valid.fsl` keeps a commented-out `on_stale`
  block (same line count, so later constructs' spans do not shift) as a record
  that the construct used to be silently accepted here.
- `expressions_valid.fsl` records accepted legacy `||` and `->` normalization;
  `legacy_logical_parse_error.fsl` records the current lexer rejection of `&&`.
  The later typed-expression migration must make any change explicit.
- `invalid_*.fsl` pins the current diagnostic kind and generated-source location
  for unknown names/members, type mismatches, unsupported operators, and broken
  expressions.
- `ai_native_cases.v1.json` is a deterministic captured prompt/spec corpus. It
  records first-pass `fslc check` success, repairs, operator/enum/generated-name
  misuse, and whether a produced diagnostic points at the attempted expression.
  It does not call an external model or set quality targets.
- `known_generated_spans` labels the current public-Kernel defect where generated
  Kernel coordinates are attached to the original domain filename. Those entries
  are evidence of a known mismatch, not approval of the reported source spans.

Regenerate a baseline only after reviewing the semantic projection, public
Kernel expression/origin projection, verifier verdict/trace, and diagnostic span.
Do not update goldens merely to make a refactor pass.

From the repository root, regenerate the versioned baseline with:

```bash
UPDATE_DOMAIN_CHARACTERIZATION=1 cargo test --manifest-path rust/Cargo.toml \
  -p fslc-rust --test domain_expression_characterization --locked
```

Then inspect the complete `baseline.v1.json` diff. A baseline update must be
paired with the accepted language or diagnostic contract change that explains
every changed projection.

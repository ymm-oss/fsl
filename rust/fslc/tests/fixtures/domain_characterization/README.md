<!-- SPDX-License-Identifier: Apache-2.0 -->

# Domain expression characterization corpus

This corpus freezes the pre-typed-expression domain frontend. It is evidence for
the later migration, not a new language contract. Update its baselines only when
an intentional semantic or diagnostic change has been accepted and documented.

- `expressions_valid.fsl` covers canonical logical operators and legacy `->`, bare
  enum members, finite membership, `can(Command)`, aggregate state references,
  scalar/field assignments, defaults, invariants, and stale-policy expressions.
- `lvalues_surface.fsl` covers root, index, and field lvalue parsing. Its current
  Map-state lowering limitation is intentionally characterized as a failure.
- `effect_saga_valid.fsl` covers expressions used by effect and saga lowering.
- `on_stale` is captured in the surface projection only because current domain
  lowering omits it; this corpus records that gap without accepting it as the
  intended language contract.
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

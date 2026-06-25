# DESIGN: `entity` / `number` in the kernel `spec` (domain vs verification bound)

## Problem

A kernel `spec` could only declare a finite domain as `type X = lo..hi`. That one
form fuses two different things:

- the **domain** fact — "there are some `Claim`s", "an `Amount` is a non-negative
  number", and
- the **verification world size** — "check the model for up to 3 claims and
  amounts 0..3".

When a design-layer spec is read as documentation, `type Claim = 0..2` reads as a
*domain* statement ("there are exactly three claims"), which is false — the `0..2`
is only the bounded-model-checking world size. This is the "domain lie": the text
that is read does not mean what it appears to mean.

The `business` and `requirements` dialects already avoid this. They declare
`entity Claim` / `number Amount` and put the sizes in a sibling `verify` block
(`instances Claim = 3`, `values Amount = 0..3`). The kernel `spec` — i.e. the
design layer — lacked both `entity`/`number` and any way to move the bound out of
`type`, so design specs were forced back into the conflated form.

## Change

`entity` and `number` are now accepted as kernel `spec` items (`?item` in
`grammar.py`). Before the spec reaches `model.build_spec`, a frontend pass
(`expand_spec_domains` in `dialects.py`, invoked from `parser.parse_src` and from
`dialects._parse_file` for `implements ... from` targets) lowers them to
`type X = lo..hi` using the `verify` block:

- `entity X` + `verify { instances X = N }` → `type X = 0..N-1`
- `number X` + `verify { values X = lo..hi }` → `type X = lo..hi`

A spec that declares no `entity`/`number` is returned unchanged, so existing
kernel specs are completely unaffected.

The lowering and its validation (missing-bound errors, entity/number conflict,
`instances >= 1`, verify bounds that reference an undeclared sort) are the **same
code** the requirements dialect uses: the shared helpers `_collect_entity_number_locs`
and `_entity_number_to_types` were extracted from `_expand_requirements_with_display`
and are now called by both paths. There is no second implementation and no new
error class.

## Why frontend desugar, not a kernel change

The kernel (`model.build_spec`, `bmc.py`, `runtime.py`) still only ever sees
`type X = lo..hi`. `entity`/`number` are pure surface syntax that disappears in
desugaring, exactly like the dialects. This keeps the verifier semantics, the
dual evaluator, and refinement unchanged, and follows the repository rule of
"prefer adding to the frontend over widening the kernel".

Rejected alternatives:

- **Annotate `type Claim = 0..2`** with a comment/tag explaining the bound — does
  not remove the lie, only documents it; and the prose is unverified.
- **Drop the range from `type Claim`** and read it from `verify` — smaller grammar,
  but it erases the useful distinction between an identity sort (`entity`, lower
  bound pinned to 0) and a numeric sort (`number`, arbitrary `lo..hi`).

## See also

- `docs/DESIGN-dialects.md` — the requirements/business desugaring this reuses.
- `docs/DESIGN-layers.md` — the business ⊒ requirements ⊒ design chain.
- `examples/e2e/3_design.fsl` — the design layer authored as readable documentation.

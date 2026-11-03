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

- `docs/design/DESIGN-dialects.md` — the requirements/business desugaring this reuses.
- `docs/design/DESIGN-layers.md` — the business ⊒ requirements ⊒ design chain.
- `examples/e2e/3_design.fsl` — the design layer authored as readable documentation.

## Native finite-domain amendment (#1480)

The original lowering discussion above describes the Python implementation;
the authoritative native implementation is the Rust workspace. This amendment
repairs existing builtin `Bool` binders, not the syntax or admission policy for
arbitrary domains. Ordinary symbolic/concrete quantifiers, concrete statement
`forall` coverage, and static `leadsTo` wrappers preserve builtin type identity
and enumerate `false`, then `true`. Named enums/ranges remain named domains;
`Named("Bool")` is not a new type alias. Qualified names still require lowering.
Aggregate Bool normalization and existing init/action binding lifetimes remain
unchanged. Success JSON remains keyed by property, not by expanded binding.

**Breaking admission removal:** a constant `within` value below zero now fails
checked-model validation at the owning `leadsTo` declaration, before engine
dispatch. Previously `check` could return `ok` while engines rejected it later.
The existing nonnegative language requirement did not make that admission
change non-breaking. Literal, named and arithmetic negative values are removed;
zero and positive values, including named/arithmetic constants, remain accepted.
Existing noninteger/state-reading constant refusals and defensive checks for
subsequently mutated negative KernelModels remain in place.

The own-tree comparison used base
`39f397d6eaeb594183dc693d5340a938e265c611` and this five-site native repair
(initially measured as `652114dca2c99dcbf137ee61ee1dbbf2ae2b4d8a` plus its
42-gross-line source diff). Reproduction builds B and the commit containing
this amendment in separate materialized trees, then runs `fslc check <path>`
on every `.fsl` under `specs/`, `examples/`, and `rust/fslc/tests/fixtures/`.
All **610** corpus files retain their check disposition: no corpus removals.
The **400** actual inline forms include all 137 shared Cases and their companion,
the new native/runtime constructors, and protected init/scope fixtures. Including
all shared engine, auto and selector contexts gives **1,489 commands per tree**;
relative companion files and working directories are preserved.

Every observed `check` `ok` → `error` transition is a breaking removal:

| Frozen source form | Deadline | New location |
| --- | --- | --- |
| shared `X1/negative-within` | `-1` | 8:3 |
| native `deadline("-1")` | `-1` | 5:3 |
| native `deadline("K")`, `const K = -1` | `K` | 5:3 |
| native `deadline("(0 - 1)")` | `(0 - 1)` | 5:3 |

The four Bool-init check error → ok transitions and three shared Bool engine
error → success transitions are repairs, not removals. No other verdict
transitions occurred in that historical comparison. Later #1480 work has its
own qualification; the Typed Struct admission policy is resolved below.
This is not universal domain agreement or full issue closure. Explicit verification
still does not support `leadsTo`; unranked induction liveness remains bounded by
the requested depth.

## Typed Struct binder policy (#1480)

A typed binder such as `forall cell: Cell { ... }` does not enumerate a Struct,
even when every field has a finite carrier. Current engines use the finite-scalar
`domain_values` rule, so expression/action validation rejects this form with its
existing `type 'Cell' is not a finite scalar` diagnostic and owning source location.
This is a **breaking check-admission removal**, shared before engine dispatch.
It does not restrict ordinary Struct values or collection binders that visit
Struct elements, and preserves Bool, named Enum and integer Range domains.
General Struct-domain enumeration is not implemented by this decision.

Init-statement Typed-Struct domains were already refused by the ownership
preflight. Preserve its complete unlocated finite-scalar error, without a new
Core prefix or source location; this form is not a new check-admission removal.
Bare Core construction and init revalidation defer only the statement-domain
enumeration check. Expression binders inside init remain strict. The ordered
checked-source preflight retains the original refusal before a pending domain
diagnostic.

The companion init-statement state-dependent-range refusal is recorded in
[checked executable forms](DESIGN-check-executable-forms.md#1480-shared-refusal-for-unsupported-binder-domains).
Ranges that read state, lets, patterns or collection elements are decided by
[checked range admission](DESIGN-check-executable-forms.md#1480-checked-range-admission);
`DOMAIN_BOUND` bounds enumerated requirement questions and does not limit range binders.
The required own-tree base/head command, SHAs, form count and actual transition
list are **pending**. Earlier finite-domain/Stage A sweep counts do not establish
this amendment's transition set.

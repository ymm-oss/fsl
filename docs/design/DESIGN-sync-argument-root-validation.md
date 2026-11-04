<!-- SPDX-License-Identifier: Apache-2.0 -->

# Root-authored synchronized arguments (#1517)

Status: **Accepted contract; implemented, product gate open (#1517).**
This amends DESIGN-compose and DESIGN-check-executable-forms. Independent review of
PR #1677 covered head `bf0f55d2c`; the fixes for its findings came after that head and
have not been independently reviewed. Neither review nor the evidence below establishes
merge, product or full-v5 completion: #1517 stays open until the product gate passes on
merged `main`.

## Admission and context

Check EVERY authored synchronized actual against its rewritten formal type
before formal substitution can erase or duplicate it, INCLUDING unused formals.
Retain the alias-resolved original expression, root action identity, source-order
ordinal, authored argument span and private root-file identity in a core-owned
obligation queue. Type it using the complete model and ROOT HEADER parameter,
state, constant and enum context, never additional-body lets/requires bindings.
Reuse the existing checker and action-parameter machinery, with authored fallback
available from the beginning: no second validator, partial model or synthetic action.
Compatible numeric carriers remain accepted; numeric range mismatch remains a
verification-time type-bound concern, not a new static range gate.

## Ownership and ranges

Add public `SyncRef.arg_spans: Vec<Span>`: each half-open range covers the whole
authored expression (including its own parentheses), from before expression parsing
through its last token, excluding comma/call close. Require
`args.len() == arg_spans.len()` BEFORE zip/collection. Malformed manual ASTs fail
closed; never truncate or default to the action span. This breaks public Rust
SyncRef constructors; inventory and update ALL constructors/tests.

Keep private root identity separate from diagnostic presentation. Clone preserves
obligations; every KernelSpec constructor initializes them; `with_source_file`
stamps/rebinds private identity with existing stamping semantics. Model construction
consumes the queue. ModelError.span uses the whole argument unless a genuine inner
span lies within this SAME actual's authored byte range; an expanded definition or
out-of-argument location falls back to the whole actual. Do not patch unknown 1:1
after checking. Root presentation uses a non-component origin with source_file=None:
bare `… at 5:16`, not `root.fsl:…`, no component wrapper/related component cause.
Existing component wrappers remain unchanged; no synthetic public registry targets.

For the regression root, `true` is 5:16–20 and `part.h[0]` is 5:16–25.
LSP UTF-16 ranges are respectively (4,15)–(4,19) and (4,15)–(4,24);
CLI/Worker loc remains the start 5:16. Original action metadata stays **4:3**.
Manual public Kernel projection keeps its old fields/bytes, not arg metadata;
current compose export refusal for component filenames is not a successful export.

## Error phases

1. Existing lowering parse/I/O/dependency/name/alias/action/arity/rewrite/hygiene failures.
2. Existing declaration/constant/type/parameter assembly failures.
3. Queued actual typing in authored action/reference/argument order, not HashMap order.
4. Existing init/component expression checks.

A later immediate alias/arity failure can therefore beat an earlier Bool actual.
There is no global authored-order promise across phases; retain competing controls.

## Validated versus raw conversion

Add fallible public `KernelSpec::into_validated_syntax(self) -> Result<SurfaceSpec,
ModelError>`: validate an exact clone through the existing FULL build_model, then
return the ORIGINAL surface only on success. `syntax`, `kernel_ast_v1`, `into_syntax`
remain raw, nonvalidating, lossy projections. `build_surface_model` checks ONLY its
supplied SurfaceSpec; it cannot recover erased unused actuals. Successful raw
projection bytes remain unchanged; intentional raw-projection tests remain raw.

Migrate the three production fslc main conversion paths: reachable counterfactual,
invariant counterfactual, and mutate Business/Requirements/Compose, retaining each
existing failure policy without unwrap/panic. The direct-Spec causal-expectation
tool is not compose-queue erasure. Each migrated invocation adds ONE FULL CORE
MODEL VALIDATION, no extra solver. Representative actual-caller cost measurement
is recorded only at whole-CLI scope, not as isolated incremental cost or free work. The seven source owners are syntax surface.rs/parser.rs,
core compose.rs/lib.rs/model.rs/typecheck.rs and fslc/src/main.rs.

These migrations are defensive: their rejection is not observable from the CLI. Each one
runs after the same command has already built the model from the same source, so that
earlier build reports a bad actual first. Evidence for the removal comes from `check`, not
from these callers.

## Breaking removals

**Breaking removal (#1517).** Any root actual that fails typing against its formal is now a
semantics error, whether or not the formal is used. Each form below passed `check` (exit 0)
on base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d` and is rejected (exit 2,
`result: "error"`, `kind: "semantics"`) on head
`d19e9939d8079042ddc768afe8550c4f402743dc`, each binary built from its own tree with Rust 1.98.0, with
`FSLC_CACHE=off`. `rust/fslc/tests/issue_1517_breaking_removals.rs` pins each form by name
with its complete packet; its exact inputs all pass on base, and skipping root validation
makes all 27 removal tests fail while the 3 migration-target tests keep passing. The rule
defines the removal; this table is the measured sample of it. Locations belong to those
test inputs.

| Group | Component action | Root reference | Head message |
|---|---|---|---|
| Internal declaration | `put(a: Int) { last = a }` | `part.put(0)` | `component "piece.fsl" failed to check (part.put: action parameter type is not a finite scalar at piece.fsl:1:78) at root.fsl:2:3` |
| Internal declaration | `put(a in 1..0) { last = a }` | `part.put(0)` | `component "piece.fsl" failed to check (part.put: action parameter range is empty at piece.fsl:1:78) at root.fsl:2:3` |
| Internal declaration | `put(a: Nope) { }` | `part.put(0)` | `component "piece.fsl" failed to check (part.put: unknown type 'Nope' at piece.fsl:1:78) at root.fsl:2:3` |
| Internal declaration | `put(a: T) { }`, struct `T` | `part.put(0)` | `component "piece.fsl" failed to check (part.put: action parameter type is not a finite scalar at piece.fsl:1:98) at root.fsl:2:3` |
| Used formal | `put(a in 0..1) { flag = a }` | `part.put(true)` | `invalid model expression: expression of type Bool is not assignable to Range(0, 1) at 5:16` |
| Used formal | `put(a: K) { flag = a }` | `part.put(true)` | `invalid model expression: expression of type Bool is not assignable to Named("part.K") at 5:16` |
| Used formal | `put(a: K) { m[a] = true }`, enum `C`, `m: Map<C, Bool>` | `part.put(R)` | `invalid model expression: expression of type Named("part.C") is not assignable to Named("part.K") at 5:16` |
| Unused formal | `put(a: K) { last = 0 }` | `part.put(true)` | `invalid model expression: expression of type Bool is not assignable to Named("part.K") at 5:16` |
| Unused formal | `unusedK(a: K) { last = 0 }` | `part.unusedK(part.h[0])` | `invalid model expression: public Kernel index requires Map or Seq at 6:20` |
| Unused formal | `unusedK(a: K) { last = 0 }` | `part.unusedK(nosuch)` | `invalid model expression: public Kernel cannot type identifier 'nosuch' at 6:20` |
| Unused formal | `unusedColor(a: Color) { }` | `part.unusedColor(other.tone)` | `invalid model expression: expression of type Named("other.Tone") is not assignable to Named("part.Color") at 6:24` |
| Unused formal | `unusedColor(a: Color) { }` | `part.unusedColor(part.Red)` | `invalid model expression: public Kernel cannot type identifier 'part.Red' at 6:24` |
| Unused formal | `unusedK(a: K) { last = 0 }` | `part.unusedK(if 1 then 0 else 0)` | `invalid model expression: expression of type Int is not assignable to Bool at 6:23` |
| Unused formal | `unusedBool(a: Bool) { }`, root `def rp(v: Int) = v == part.last` | `part.unusedBool(rp(true))` | `invalid model expression: expression of type Named("part.K") is not assignable to Bool at 6:23` |
| Duplicate formal | `put(a: K, a: K) { last = a }` | `part.put(true, 0)` | `invalid model expression: expression of type Bool is not assignable to Named("part.K") at 5:16` |

The internal-declaration and used-formal components also fail their own standalone `check`;
the unused-formal components pass it, and only the root actual is wrong. The duplicate-formal
row also fails standalone; its first actual is erased by substitution, and `part.put(0, 1)`
for the same formals still passes `check`. The test also pins further unused spellings of
these classes: a root `def` whose result or body does not type (`rp(0)` for `a: K`, `rb(0)`
reading a relation index), `Color.Red`, `part.Color.Red`, a name bound only by an additional
`let`, a pattern binder (`if part.o is some(v) then v else 0`), a bare component state name
(`last`), the formal's own name (`a`), a root Bool state (`rb`), an integer for an enum
formal (`0` for `a: Color`), and parenthesized and multi-line `true`. No measured form went
from error to ok.

FSL has no owner-qualified enum literal in source. `part.Red`, `Color.Red` and
`part.Color.Red` are rejected as untypable identifiers in a root actual for a used formal, a
root `requires` and a root invariant on both base and head, as `E.R` is in a direct spec and
`St.Open` in a refinement map. Base accepted them only as an erased actual for an unused formal, so this removal takes
away no valid enum spelling. The bare member (`Red`) stays valid.

Migration: pass a value of the formal's type, using a header parameter when it comes from
outside the root, or correct the component. There is no automatic migration, and an
additional-body `let` still cannot bind an actual.

Earlier characterization pinned eleven exact byte contexts over nine root/component file
pairs, in both stable and FSL5 opt-in modes (22 results), all exit 0 to semantics error exit
2. The nine exact three-file `check c.fsl` contexts measured with them show preservation
only: a `direct.fsl` file's presence there gives no direct, API or verify coverage. These
are a finite sample, not every witness in the issue and not issue acceptance. Their bytes
are identified by hash and were not retained, so they were last measured on base
`5564b25ccb37e3477311fde1dae5eb6298eb6b2d` and head
`7653cc784354bc562bd85bd888cf78d0091272f2` and not rerun on the head above.

| ID | Exact complete-context key | Pair | Changed admission / candidate loc |
|---|---|---|---|
| R01 | `1cae9cf75cb49d332ab850296a1902db6e08971cbae68ab7229309b979632f05` | P01 | unused true + relation index, argument order; 5:16 |
| R02 | `456840abf00faf2844ad3223e80abab2efb03e62ec5c2fc4c6c5cef9daa45af4` | P02 | body-only x after let; 5:16 |
| R03 | `5aa3baefa078f9ddc0a1d5ed5157e9bd6295df700bf86536d7042d774e69e0d2` | P03 | true then later action index; 5:16 |
| R04 | `8d7b0467683a00853cc958fcf91c33e6e23d32c7cd5506d1401029e98338b5d9` | P04 | relation index; no good sibling; 5:16 |
| R05 | `a1760ef370b7c9164f0eda14b36c84dfa208b847c75fb6ecb8bfa8e5faa8df22` | P05 | unused true; no good sibling; 5:16 |
| R06 | `ab059cd5d0c142246d68ccb9ce2ea193e43ce9b4365e6a865e0a7b46c63942ac` | P06 | true then second reference index; 5:16 |
| R07 | `ab291946a038aa0e77692160e7516fa40a75b8781797c928405dddfd8949bbbe` | P04 | relation index + offered good sibling; 5:16 |
| R08 | `acc62ea03d44de4dd0ddbcd65d4f73e4737e914153f50bed7832a4bb4905da3b` | P07 | body-only x after let/requires; 5:16 |
| R09 | `cd076b93cba275c70735ba2a73731c1935fabcdb0fe16f3246b5ba615e720ded` | P08 | parenthesized unused true; 5:16 |
| R10 | `dd449ab57104f7c5a53e33d3f756937344e76161615f960cb821e2e62372adff` | P05 | unused true + offered good sibling; 5:16 |
| R11 | `91412ae303f1f1877fcdcea2e7e35912726e9832662e0c49d971d8ca09a9bc02` | P09 | second ref `(0), (true),`; 6:37 |

All entries are `root.fsl` declaring compose Root, with `piece.fsl` declaring
spec Piece, K=0..1, `h: relation K -> K`, `last: K` initialized to 0, and unused put formal(s)
whose body is `last = 0`. P01/P09 have two formals; P06 adds second(a:K) {}; the
others have one formal. Other declaration/spacing differences remain identified
by these exact byte hashes, not normalized away.

| Pair | Exact root SHA256 | Exact piece.fsl SHA256 |
|---|---|---|
| P01 | `e034202ed5365c3076ca51b6e9c8e1513926717b045dc9de47e2953a0343dd3b` | `f5d9050744bb33c195afa70fad64c390583898565507c62144df19cf750afa86` |
| P02 | `5c6c7f1391941f97e0af91b5c1ca01ea6fe4edc792bb2ea1009789c6a3516f93` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P03 | `e61b94ebe2f2aba93df7b464afb94cfe756e353527054721798c6f7c49417d35` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P04 | `73e678576bcc1d25cb475911a1406a08cc8dceaf2344fb9af5b1b5d90eb52038` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P05 | `0e713efcd02ef1e3e3d6a9c3619d8e0ffa7db26196b6b7182b2e5982e12a4907` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P06 | `64fefc9802089548b6fa2743cf3172264585ac869f3914e1fb1cd4834326ec53` | `7dcc8919022832f96b50dfbcd7c7b5c6b009dea6fb7e8740f360e2cfa672af75` |
| P07 | `5e92fd04a07d15a99a0612920911153ede546ed2c2e95dd3030c4d01e094271b` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P08 | `40b3935dec0b76b3d5e6569b76cd98559b190939be45adae98764da57e5328af` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P09 | `fb95cf70d9a759ada865a3d83260f867dbfea4634ef12129c7699399eaff8fd9` | `f5d9050744bb33c195afa70fad64c390583898565507c62144df19cf750afa86` |

The exact good.fsl SHA is `7a79980377068e1567fd3394a3f214323e76297897906dba63a7cbcfa532889c`,
declaring spec Good with `seen: Bool` initialized to false and `idle() {}`. It is offered
but unreferenced in R01/R02/R03/R06/R07/R08/R09/R10; R11 references it as first
and calls first.idle() before part.put. R04/R05 contain only root.fsl+piece.fsl.
All other contexts retain all three files. Read roles and presence are distinct.

R04/R07 and R05/R10 each share root and piece bytes but differ in the offered good.fsl, so
all eleven contexts stay separate.

On-disk transition set (empty). Base `5564b25ccb37e3477311fde1dae5eb6298eb6b2d` and head
`d19e9939d8079042ddc768afe8550c4f402743dc`, each built from its own materialized tree with Rust 1.98.0, the CI toolchain
(`cargo +1.98.0 build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked`). Every
`.fsl` under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` (610 forms on both
sides: 23/195/392) ran with `FSLC_CACHE=off` and the own tree as cwd, as
`fslc check <path>` stable and with `FSLC_UNSTABLE=fsl5`, and as
`fslc kernel <path> --kernel-version 1|2` stable, twice per side. All 4880 pairs had equal
complete stdout, stderr and exit, with no field excluded: 0 ok-to-error and 0 error-to-ok
transitions. Kernel version 2 records span paths relative to the repository root, so a
materialized base tree needs a repository root too: without one, 428 forms differ only in
that path. The inline `s.fsl` of the rewritten
`breaking_removal_sync_arguments_do_not_read_additional_lets` stays an error and moves from
the component wrapper at 2:3 to the root at 4:21; its other four inputs are byte-identical.
This covers `check` and Kernel on these forms, not every verifier route.

Multiple actual, reference and action order, body-only `x`, parentheses and the trailing
comma are part of the removal; they give additional-body lets no permission to bind actuals.

A conditional with a Bool, ranged or named expectation (a numeric domain, an enum or a struct)
checks its arms against that expectation, so `m[if c then 0 else true]` with
`m: Map<A, Bool>` names `Named("A")` rather than `Range(0, 1)`. Every other conditional keeps
the base check of its arms against the first arm's inferred type. That includes an `Int`
expectation, where the supplied type would only hide a more specific first arm, and every
container. Compatibility is an equivalence over scalars but not over containers holding
`Int`: `Set { 0 }` is compatible with both `Set<A>` and `Set<B>`, which are not compatible
with each other. Applying the expectation to container arms changed admission in both
directions, so it is not done.

This does not change admission on the measured forms. Against base on the head above: the
four forms pinned in `issue_1517_conditional_arm_admission.rs` give equal results on `check`,
bmc and explicit; of 79 finite conditional probes, 77 give equal packets and 2 differ only in
the named type; of a 5917-form matrix of Set, Seq, Map, Option, relation and scalar arms in
`requires`, assignment, index, method and operator positions, 1870 are ok on both sides, 4047
are errors on both, and 126 of those errors differ only in the type named at the same
location, for example `Range(0, 1)` to `Named("A")`. As a control, the same matrix on a binary
that applies the expectation to every conditional shows 48 admission differences. This is a
finite measurement, not an equivalence proof.

## Evidence and open gates

Implementation evidence must cover used and unused rejection, valid header, alias, forward
const, state and numeric controls, missing or swapped metadata and queue omission, Clone,
stamping and rebinding, validated conversion accept and reject, phase competition,
parentheses, inner spans, UTF-16 over real stdio, complete native and Worker packets, and
model, manual-AST and Kernel preservation or exact refusal. The tests written for this are
the `issue_1517*` tests of `fsl-core`, `fslc-rust`, `fsl-wasm` and `fsl-lsp`; with all
`fsl-core`, `fsl-wasm` and `fsl-lsp` stdio tests and the `issue_1442*` tests of
`fslc-rust`, they pass on head `d19e9939d8079042ddc768afe8550c4f402743dc` with Rust 1.98.0.

On `c5f48d81b`, before the `Int` exclusion and built with Rust 1.99.1, replacing the three
migrated callers with raw conversion and rebuilding changed nothing the CLI shows on one bad
and one valid compose through `check`, `verify`, `explain` and `mutate`, apart from
elapsed-time values. An earlier candidate measured the same on fixed valid inputs, and exact
restoration preserved its output. Together these show preservation only: no caller-specific
rejection and no isolated cost of the added validation.

Still open before issue or product completion: independent review of the fixes after
`bf0f55d2c`, the required workspace and product gates on merged `main`, and #1254/#1622
preservation on that state. Not measured here: repeated isolated core faults and verifier
routes other than those named above.

# FSL v2.0 — Implementation Bridge (runtime monitor / replay / testgen) Implementation Design

Status: migration-era bridge design with later compatibility updates. Its
execution sections record the original v2.0 design and remain useful history,
but they are not current product authority. The native
Rust ownership map is in
[`DESIGN-rust-components.md`](DESIGN-rust-components.md), and current transition,
failure-order, and rollback authority is in
[`DESIGN-kernel-contract.md`](DESIGN-kernel-contract.md#transition-and-failure-semantics).

DESIGN-v1.md §10 v2.0 "the body of the implementation bridge." Three bridges are
built between the spec and the implementation:

1. **`fslc.runtime.Monitor`** — a concrete interpreter of the spec (no Z3, pure
   Python). Embed it in the implementation to use it for runtime conformance
   checking (runtime monitoring).
2. **`fslc replay`** — a CLI that checks an event log of the real system against
   the spec. The first entry point usable without writing code.
3. **`fslc testgen`** — generates a conformance-test (pytest) skeleton for the
   implementation. Replay tests of scenarios (existing) + random-walk property tests.

Design principle: because it is a **second implementation with the same semantics
as the verifier (the Z3 evaluator)**, the two cross-check each other via
**differential testing** by replaying witness traces (§6).

## 1. `fslc.runtime` — the concrete interpreter

A new module `src/fslc/runtime.py`. It does not import Z3 (`parse`/`build_spec`
may be used — if model.py imports z3, we do **not** require going as far as "z3
unnecessary at runtime use" by making the sort-construction part a lazy import,
etc. In v2.0, use within the same package is sufficient).

### 1.1 Value representation (logical values — the same convention as verify's JSON display)

| FSL type | Python representation |
|---|---|
| Int / domain type | int |
| Bool | bool |
| enum | str (member name) |
| Option | None or the contained value |
| struct | dict (field name → value) |
| Map | dict (logical value of the key → value). The key is total (enumerate the bounded keys and initialize) |
| Set | Python set |
| Seq | Python list |

### 1.2 API

```python
from fslc.runtime import Monitor

mon = Monitor(spec_source_or_path)      # parse + build_spec + static checks
mon.reset()                             # -> state dict (initial state)
r = mon.step("checkout", {"u": 0})      # -> result dict (below)
mon.state                               # current logical state (dict, JSON-compatible)
mon.enabled()                           # -> [{"action": str, "params": dict}, ...]
```

The result of `step` (the same vocabulary as verify's JSON):

```json
{ "ok": true, "state": { ... }, "changes": { "stock[0]": {"from": 1, "to": 0} } }
{ "ok": false, "kind": "requires_failed", "action": "checkout",
  "params": {"u": 0}, "requires": {"loc": ..., "text": "..."}, "state": { ... } }
{ "ok": false, "kind": "ensures" | "type_bound" | "partial_op" | "invariant",
  "name": "<invariant name or _bounds_* or _partial_*>", "loc": ...,
  "state": { ... }, "hint": "..." }
```

- On violation, **do not change the state** (requires_failed / partial_op /
  type_bound / ensures / invariant all roll back before the transition). No
  exception is thrown — always return a result dict (easy to embed as a monitor,
  and readable by the LLM).
- The historical bridge design modeled `step`/BMC as evaluating all requires (no
  short-circuit) → apply the body by simultaneous assignment (all RHS read the
  old state) → partial_op check (considering path conditions) → ensures
  (old = old state) → invariant of the new state + automatic bounds check.
- That sequence is retained as migration history, not as the current contract.
  Current guard and outcome priority must be read from the maintained Kernel
  transition/failure contract linked above, not inferred from this paragraph.
- An unknown action name, a missing parameter, or being out of type range is `kind: "bad_call"`.
  A `forbidden` last step narrows this: only an argument outside the parameter's declared
  range or enum type is a `bad_call` rejection there; an unknown action or arity, and an
  argument outside an `entity` / `number` verify scope (#1229), is a check error
  (DESIGN-forbidden.md §2).

### 1.3 Determinism of init

Concrete execution requires a deterministic initial state. Static check at
`Monitor` construction:

- init assigns to every state variable **exactly once** (forall bulk assignment allowed).
  For a Map/index target (`m[K] = ...`), "once" is per concrete key,
  including a key a `forall`-bound variable already covered: flat
  `m[K1] = ...` / `m[K2] = ...` statements for two *different* keys are not a
  duplicate (this is exactly how a dialect like fsl-db populates a map one
  column at a time), but the same key assigned twice — whether by two flat
  statements, by a `forall` and a later flat write to one of its covered
  keys — still is. A finite binder-keyed `forall k: K { m[k] = ... }` is
  allowed. Ownership summarizes each statement once, not once per iteration:
  a single forall statement may write the same location repeatedly with agreeing
  values, while conflicting values make init unsatisfiable.
- The RHS of init may reference only const and **already-assigned** state
  variables (evaluated top to bottom). Native errors use `kind: "semantics"`
  and the before-assigned message (exit 2); the historical Python deterministic-init
  hint is not part of the native output contract.
- All existing specs satisfy this (a spec that does not satisfy it passes verify
  but errors in Monitor — prompting a fix on the spec side).
- This gate lives in `Monitor::new` itself (#519), not only in the explicit
  engine's own construction check (`explicit::check_deterministic_init`, which
  `Monitor::new` now reuses) — every caller that asks `Monitor::new` to compute
  the concrete initial state from `init` gets it, including `replay`'s witness
  reconstruction and concrete boundary/conformance tooling.
  Symbolic BMC performs its action-context partial-operation check within
  `verify_bounded*` and does not construct a Monitor first (#651). A caller
  that already has its own complete concrete initial state — an
  observed replay trace's own step 0, an explicit `--from-state`/
  `--initial-state` snapshot, or a BMC witness's first state — is not asking
  `init` to compute anything, so it builds through `Monitor::from_state`
  instead and is unaffected by this gate: `init` may leave a component free
  (BMC explores every admissible value there), and a concrete run pinned to
  one already-admissible value has nothing left for the gate to check.

#### Init forall read order and coverage (#1518)

The native runtime shares a read-order check between init ownership validation
(`check` and symbolic verification) and deterministic-init validation
(`Monitor::new` and explicit verification). In a statement `forall`'s filter
or body, a read of a state root written by that same `forall` is rejected,
including reads in assignment keys, right-hand sides, and `if` conditions.
Only a location at the current binder key that has already been written in
the current iteration is allowed. Writing `m[k]` permits later reads of
`m[k]` and `m[k].flag`; writing `m[k].flag` permits a later read of that field,
not the entire entry or another field. A write on only one `if` branch is not
a definite preceding write after the `if`. This does not introduce new
assignment shapes or relax existing ownership checks.

For example, `forall k: K { m[k] = true seen[k] = m[k] }` remains valid;
replacing the read with `m[1]`, or moving the read before the write, is now
rejected. A read of a different root initialized before the `forall` remains
valid. This is a **breaking removal of check acceptance**, including scalar-map
and whole-struct-entry forms; rejection is root-based, not restricted to
symbolic/concrete iteration-order disagreement. The semantics message identifies
the root, not the individual read expression.
Check and verification retain the enclosing statement's location (assignment,
`if`, or `forall` for a filter); legacy replay retains its existing error
envelope without a required location.

The removal is intentionally conservative, not a per-key dependency proof.
For example, `forall k in 0..0 { m[k] = true }` followed by
`forall k in 1..1 { m[k] = m[0] }` is rejected: the second forall reads a
different key of the same root it writes, even though the preceding forall
initialized that key. Likewise,
`forall outer in 0..0 { forall k: K { m[k] = true seen[k] = m[k] } }`
is rejected even immediately after the inner write: the outer forall also
writes `m`, and the read is not at its binder key. Both forms were already
refused by explicit and Monitor on the base, with `before it is assigned`
and `nested forall in init is not supported`, respectively; #1518 also
rejects them at check. The rule is not refined to admit these cases.

It also removes correct forms whose base-engine results agreed: with `m`
indexed by `0..1`, `m[0] = true forall k in 1..1 { m[k] = true x = m[0] }`
reads an already-initialized other key, but is rejected because the loop
writes the same root.

Definite coverage and write ownership are distinct. A forall whose domain is
empty, or not statically provable nonempty, contributes no definite coverage
for its fixed targets. Static filters may establish nonemptiness using only
the binder and constants (for example `where k == 0` over `0..1`). A
state-dependent filter is not such a proof, even when its state was initialized
earlier. Ownership validation still inspects the body, and the existing
conservative binder-key coverage classification is unchanged. Thus an
unexecuted write cannot initialize state computed by `Monitor::new` or explicit
through an arbitrary default. Refinement's free-root `concrete_initial_states`
path does not use `Monitor::new`: it checks ownership and read order, then
refuses incomplete assigned roots on either side (#1521), including forall domains
not proven nonempty. Partial-component enumeration is future work (#1579).
Explicit and Monitor reject missing deterministic assignments with exit 2;
symbolic init retains its free-value semantics, so it may violate a type bound
or invariant with exit 1, or verify when all admissible values satisfy the
property. Auto falls back from unsupported explicit init to BMC as before.
This is also a conservative concrete-support breaking removal: after
`flag = true`, a fixed-target write under `where flag and k == 0` is refused
by explicit and Monitor with `init does not assign` even when its runtime
domain is nonempty, rather than producing an incorrect verdict.

A separate pre-existing explicit limitation remains: for a where-filtered or
collection binder, reading the current key after writing it is allowed by the
#1518 read-order rule, but explicit and Monitor still refuse with
`before it is assigned` because their binder-key coverage is conservative.
This refusal already occurred on the base; it is an unsupported concrete-init
shape, not an incorrect verdict. `--engine auto` falls back to BMC for it.
Improving that binder-key coverage is tracked in #1520, not part of #1518.

Breaking-removal sweep (#1518): base
`28c4eab710f8d83e766bb0e7c7e006d09d7777d8` versus head
`9d94c5cb5447834f329002200afa0f3404d9a9a0` plus the production runtime change
(`explicit.rs` SHA-256
`b20b92e9945fd7a05eb8462c4a7ad3816fcd06cd2c34a871910b7b652090c17b`).
The coordinator built each native binary in its own git-archive materialized
tree and ran `fslc check <form>` over every `.fsl` under `specs/`, `examples/`,
and `rust/fslc/tests/fixtures/`: 597 forms, 397 `ok` in each tree,
**0 `ok`→`error`, 0 `error`→`ok`, and 0 other changes**. The corpus removal set
is empty; A1–A5 pin five regression forms within this removal, which also
includes the conservative examples above. No corpus spec was rewritten.

#### Per-field init ownership and coverage (#1481)

Native init ownership tracks `(root, concrete key, field)` (#1481): disjoint
fields of a struct root or a `Map<K, Struct>` entry can be assigned separately,
including by a finite `forall`; fields retain their scalar or nested
`Option<scalar>` types. Overlapping writes by different statements on a path
still collide, including whole-root/entry writes overlapping a field. Repeats
across iterations of one statement retain the relational-init rule above, not a
second ownership identity. Unresolved dynamic keys and binder keys under a
`where` filter, collection binders and non-constant range binders retain
whole-root ownership and conflict with other writes to that root. Definite
assignment keeps recursive coverage per map key and per struct field, joins successive writes and intersects
`if` branches component-wise. Only complete coverage of every field of every
entry permits a root read or concrete Monitor construction; these deterministic
paths reject partial initialization rather than filling it from defaults.
Refinement also refuses incomplete assigned roots on either side (#1521),
including a single-field partial init, rather than defaulting missing components.
bmc and induction retain their free-value verdicts; partial-component enumeration
is future work (#1579).
The existing concrete field updater and symbolic init constraints consume these same checked lvalues;
no solver dependency is added to the runtime.
These component identities remain subject to the forall read-order and
nonempty-domain coverage rules above; they do not bypass either gate.

Admission of a shape requiring the new per-field ownership first requires full
definite coverage of every root init may assign on every path, including roots
written only as whole values. This is the same may-assigned set refinement
excludes from free-root enumeration; never-assigned roots remain free.
The final coverage
uses the recursive key/field joins, branch meets and nonempty-domain rollback,
not a union of field names written somewhere. Missing keys or fields retain the
legacy probe's original ownership error and location, preventing new partial
initialization from entering refinement's free-root path. The legacy probe still
exempts all previously accepted partial forms (including branch-split fields,
whole-entry key subsets and one-path whole-root writes) from this ownership rule.
Refinement separately refuses incomplete assigned roots (#1521), while bmc and
induction retain free-value verdicts. Partial-component enumeration is future
work (#1579).

Only the new admission/read analysis uses scope-aware static proofs. A forall
source, filter or range bound that freely depends on an enclosing binder cannot
prove nonemptiness or binder-key values using a same-named global const. Its
coverage is unproved and rolled back, or its keys remain unresolved, yielding
the saved legacy refusal. Sources use the enclosing scope; a binder's own filter
and nested quantified/aggregate bodies use their local scope. No enclosing
valuation combinations are enumerated. The legacy ownership/concrete paths and
the symbolic resolver retain their existing behavior; #1515 is not fixed here.
For outer-dependent ranges, nonempty rollback is the decisive coverage refusal;
the bound resolver also leaves their keys unresolved, but that guard alone does
not change admission while the rollback remains in place.

For admitted new shapes, check also requires a complete may-assigned root before
reading it, including roots written only as whole values. Early reads of
non-field-written roots return the saved legacy ownership error and location
unless the higher-priority field-read gate refuses first, even at a later
statement. This is pass priority, not a globally earliest-read diagnostic.
A Map needs every key and every field, not merely the selected entry.
Whole-entry or whole-root reads of a field-written
root follow the same rule, with `<whole entry>` / `<whole root>` diagnostic markers.
This rejects `s.a = 1; s.b = (s.a == 1)` and
reads of an already-written `m[k].f` before its siblings in the forall body;
reading an already-written field at the current binder key after all binder-keyed
fields are written remains valid. Whole-entry copies inside the body still fail
#1518's field-path rule, which does not promote separate field writes to a
whole-entry write; whole-entry copies outside the loop after full-root init are
allowed. The same recursive coverage joins, branch meets and nonempty-domain
rule feed this gate. A keyed write credits all statically known binder keys at
once; this relies on #1518's separate forall read-order walk running first and
limiting in-body reads to the current already-written key/path. These walks must
not be reordered.
Errors name the root and field and use the containing assignment, if or forall
span, as the #1518 read-order gate does. The implementation first validates
per-field ownership, then probes the previous root-granularity ownership rule;
only a form that needs the new ownership granularity runs the full-coverage
admission and extra all-assigned-root read checks. Previously accepted shapes
retain their ownership-check admission. Refinement checks ownership and read order,
then refuses incomplete assigned roots before replaying init in program order
(#1521), on both implementation and abstraction sides. Never-assigned roots remain
free and are enumerated; bmc and induction retain free-value verdicts for partial
init. Partial-component enumeration is future work (#1579). This admission-only
rule does not itself reject legacy-accepted whole-value reads before assignment;
the separate #1451 gate below addresses those reads.

Concrete coverage now counts resolved map-entry-field writes even when the legacy
ownership probe succeeds. A complete map of single-field entries can therefore
be constructed by explicit/Monitor where main reported missing assignment or an
early root read. Retained but incomplete entry-field coverage adds the existing
partial-component suffix to the missing-assignment error; complete coverage can
also remove the map from the missing-root list. Branch coverage still intersects
keys and fields, missing arms drop the root, and unproved nonempty loops roll
back their contribution. Unresolved entry keys, root-field coverage, nested-forall
restrictions and root-wide concrete reads are unchanged. These are intentional
concrete changes, not a promise to preserve every legacy concrete error/result.
Complete coverage need only contain every declared key: a superset such as
`forall k in 0..2 { m[k].v = true }` for keys `0..1` now reaches the existing
out-of-domain execution refusal instead of missing assignment. Repeating a
fixed singleton-map entry's field write in one forall statement can likewise
reach the existing unsatisfiable-init/vacuity refusal when iteration values
conflict. A one-iteration fixed-key write becomes constructible. Relational
repeated-write constraints are unchanged; their concrete refusal stage/kind is
not. Mixed whole-entry/field branches meet `Full` with field coverage: singleton
maps can become full, while larger maps retain partial coverage and its suffix.

The consumer boundary includes every use of this concrete initialization, not
only explicit verification and empty Monitor replay:

- Refinement with no free roots uses `Monitor::new` on both the implementation
  and abstraction sides. An old constructor error can become `refines` or a
  genuine mapped-init `refinement_failed`, according to the mapping.
- `verify --engine auto` can replace unsupported-init fallback to bounded BMC
  with explicit unbounded closure. On the superset-key example it instead
  replaces BMC's located message (`in init at 5:29`) with the existing unlocated
  explicit out-of-domain message. This diagnostic loss is part of the change,
  not an unchanged-location guarantee.
- `conformance` can now emit initialized vectors; `testgen` can emit a test
  module instead of its existing constructor error (exit 3/internal). Their
  prechecks differ: symbolic init errors can still preempt test generation.
- Requirement acceptance/forbidden validation constructs a Monitor per case.
  Newly constructible init can change `check`, proof and testgen outcomes: a
  valid trace passes, while an incorrect expected value now reaches a located
  acceptance rejection instead of an init-construction error. Kernel empty
  replay and conformance vectors do not themselves validate the document's
  declared acceptance cases.

Thus kernel ownership admission and relational constraints can be preserved
while consumer verdict, engine/completeness/fallback, diagnostic stage/kind/
location and generated artifacts change. Four-engine agreement alone is not a
complete compatibility statement for these consumers. The runtime remains
independent of solvers.

Diagnostic priority distinguishes the two root sets. If a field-written root is
itself incomplete, return the saved legacy ownership error first. If all
field-written roots are complete, run gate(a) before the final completeness
refusal for other may-assigned roots, so a field read retains its named
message and statement location. Successful admission still requires every
may-assigned root to be complete. After gate(a), early reads of other may-assigned
roots retain the saved ownership refusal, as do incomplete assigned roots. A
later field read therefore outranks an earlier nonfield read; the concrete
legacy preflight does not replace that field diagnostic.

On a shared ownership refusal, concrete construction preserves main's earlier
program-order read or domain failure, if one precedes the legacy collision.
A private legacy-concrete preflight reproduces that ordering, including the
existing nested-forall refusal; it only chooses an error and never constructs a
state. New gate(a) and #1518 read-order diagnostics retain priority when they
are the shared failure; gate(a) intentionally changes concrete messages on those
main-rejected shapes. Check/refine report the saved ownership error for other
admission refusals, including early reads of non-field-written roots.

#### Assigned-root read order (#1451)

After the existing ownership/field/forall gates accept an init, the shared
runtime admission gate also checks ordered reads of every root init assigns
anywhere, in assignment values/keys, if conditions and forall filters/bodies.
Forall sources retain their separate existing validation. It reuses
`walk_init` and the concrete root-coverage predicate and
message: `init references state variable '<name>' before it is assigned`.
Native check and ordinary verification return semantics/exit 2 with the
containing statement's location. Compose, Worker check/verify and refinement's
free-root constructor reach the same gate; runtime remains solver-independent.
Existing shared ownership/field/forall refusals keep priority. This is a
source-acceptance breaking removal, not a totality check.

Coverage is whole-root, even for an already-written key/field, a syntactically
present but short-circuited read, or a value left free on another branch or
component. Thus `struct-other-partial`, `if-one-branch-read` and #1481 N2 now
reject reads of #493-style free values because their root is may-assigned.
A root never assigned anywhere remains free, including the issue's `on` and
the `choose` condition of nondeterministic refinement init (#493). Check and
symbolic verification retain those forms, while explicit still refuses them.
Only unresolved binder values retain #1520's same-forall current-key exemption,
subject to #1518's already-written-key rule, including filtered/collection
X1/X2. Resolved binders use actual whole-root coverage: a full-domain typed
loop is accepted, but a loop over only `0..0` cannot read its current `m[k]`
when `m` has keys `0..1`, even if the remaining keys are written later.
This does not promote the unresolved forms' explicit support.
Binder-keyed filtered/collection writes do not establish the complete key set
for a following read. Nesting an unresolved inner binder does not restore that
coverage. This does not remove static nonempty proofs for fixed targets.
Unread omitted roots and legacy-accepted partial init remain
accepted. The expression-only read exemption never credits real assignment
coverage or changes the initialized state.

Sweep command: run `fslc check <form>` with the base/head binaries in each
revision's own git-archive materialized tree. Measured base
`c79016efeb630c717292e9392901cfe65ab40915`, head
`2d05dd63f05ca68c4bd7d06c3b24ba147ce6eda4`; binary SHA-256 respectively
`e42827307a61957e16d6a53e12bf84216506fc7eaa2a25ccc3a0084874b748f2` and
`15ae6af290ba67b7fa8801494ee62fe54b8def11dc1393ba9067553a78953a8b`.
All **597** `.fsl` files under `specs/`, `examples/`, and
`rust/fslc/tests/fixtures/` have identical full stdout/stderr/exits (400 exit 0,
191 exit 2, 6 exit 1): corpus `ok`→`error` set **empty**. The separate inline
sweep covers the 36 author sources, N2 and five implementer fixtures, each
direct and composed: 42 sources / 84 forms, base 80 exit 0 + 4 exit 2, head
42 exit 0 + 42 exit 2. Exactly **38** forms become errors: the original 13 cases
× 2, N2 × 2, the two static-subset cases × 2 and the three post-loop coverage
cases × 2. Those are also the only full-output changes; no fields were
excluded. N2 is not counted among the 597 corpus files.

The table lists the targeted inline removal fixtures and a migration example
for each; it is not an exhaustive list of all removed programs. It includes
the original cases and the resolved-subset / post-loop coverage families.
Bodies use `K = 0..1`, `R { on: Bool, other: Bool }`,
and the corresponding regression's state declarations. Move reads after all
required assignments or replace partial writes with a whole-value assignment;
do not add a second overlapping write. Choosing a formerly free value, or
filling a missing branch, intentionally restricts the model and is not an
equivalence-preserving automatic repair.

| Rejected source | Replacement init body |
|---|---|
| `root-value-before` | `on = false g = on` |
| `root-unary-before` | `on = false g = not on` |
| `if-condition-before` | `on = false if on { g = true } else { g = false }` |
| `forall-body-before` | `on = false forall k in 0..1 { m[k] = on }` |
| `map-index-read-before` | `m[0] = false m[1] = true k = 0 g = m[k]` |
| `struct-root-before` | `r = R { on: false, other: true } g = r.on` |
| `conditional-value-before` | `flag = true on = false g = if flag then on else false` |
| `dead-rhs-before` | `on = false g = false and on` |
| `map-key0-partial` | `m[0] = false m[1] = true g = m[0]` |
| `map-key1-partial` | `m[0] = false m[1] = true g = m[1]` |
| `struct-on-partial` | `r = R { on: false, other: true } g = r.on` |
| `struct-other-partial` | `r = R { on: false, other: true } g = r.other` |
| `if-one-branch-read` | `flag = false if flag { on = true } else { on = false } g = on` |
| `1481-N2` (additional inline fixture) | `forall k: K { m[k] = Cell { flag: false, n: 0 } } done = m[0].n == 0` |
| `static-subset-then-fill` | `m[0] = true m[1] = false forall k in 0..0 { seen[k] = m[k] } seen[1] = false` |
| `static-subset-only` | `m[0] = true m[1] = false forall k in 0..0 { seen[k] = m[k] } seen[1] = false` — assigning formerly free `m[1]` and `seen[1]` narrows the model. |
| `filtered-followed-read` | `forall k: K { m[k] = true } g = m[0]` |
| `collection-followed-read` | `forall k: K { m[k] = true } g = m[0]` |
| `nested-filter-followed-read` | `forall k: K { m[k] = true } g = m[0]` |

For N2, `m[0].n` has no init assignment although root `m` is written. Its old
check success and symbolic bounds violation are replaced by the root-read
error; explicit/Monitor already refused with the same message. Native
check/verify now locate the read at 5:44; legacy Monitor replay renders the
same existing message without a location. The fixture is unchanged, only its
test expectations change. Likewise, `issue-exact` is reclassified as a
free-root preservation control rather than a refusal detector: `on` is never
assigned anywhere. This is Part of #1451, not completion of that issue.

Behavior notes: already-refused explicit inputs can gain a statement location
(for example an unlocated early read becomes 3:10), while a never-assigned
root's explicit refusal stays unlocated. With both a free and a later-assigned
root, the named root can change (`choose` to `on`); a permitted unresolved
current-key read beside a later-assigned root can similarly change `m` to `on`.
Nested inputs can change from `nested forall in init is not supported` to the
root-read error. Source admission can also preempt symbolic/auto diagnostics
for outer-dependent nested ranges without changing their error exit.
Compose runtime errors use physical names such as `part__x` and the component
statement's line/column without a filename, like existing ownership errors.
The remaining free/choose engine disagreement is tracked on #1451: check and
symbolic verification retain these free-value forms, while explicit refuses them.

### 1.4 Expression evaluation

Implement the concrete evaluator `eval_concrete(expr, state, binds, spec, old_state)`
inside `runtime.py`. It takes the same AST as bmc.py's `eval_expr` as input and
evaluates with the Python values of §1.1. Note the following:

- Quantification/aggregation (forall/exists/count/sum) loop straightforwardly over a bounded enumeration.
- The binding of `is some(x)`, `min/max/abs`, and equality of struct/Seq/Set are over the representation of §1.1.
- Out-of-range `at`/`head`/`pop` on a Seq is **reported as partial_op at the call
  site** (the path condition is naturally protected by if-evaluation — in
  concrete execution only the branch actually taken is evaluated, matching BMC's
  "path-condition implication").
- There is no division by zero etc., but an unexpected evaluation error is wrapped in `kind: "internal"`.

## 2. `fslc replay` — conformance checking of event logs

The original `--trace` form below accepts already-normalized spec actions. For
production JSONL whose action/state names differ from the spec, use
`--from-log ... --mapping ...`; that extension reuses refinement mapping syntax
and is specified in `DESIGN-log-replay.md`.

```
fslc replay <file.fsl> --trace <events.json>
```

Input (the assumed shape of the log the real system emits — the same shape as scenarios' steps):

```json
{ "events": [ { "action": "add_to_cart", "params": {"u": 0, "i": 1} }, ... ] }
```

A JSON whose top level is just an array (`[ {...}, ... ]`) is also accepted.
These two shapes are the explicit unversioned compatibility adapter. External
compilers use the closed, versioned, complete-state contract in
[`DESIGN-replay-trace.md`](DESIGN-replay-trace.md); `testgen-trace.v1` below is
a distinct generated-test oracle and is not accepted as replay input.

Output:

```json
{ "fsl": "1.0", "result": "conformant", "spec": "ShoppingCart",
  "steps_checked": 12, "final_state": { ... } }
{ "fsl": "1.0", "result": "nonconformant", "spec": "ShoppingCart",
  "failed_at_event": 4, "violation": { ...the step's ok:false result... },
  "state_before": { ... },
  "hint": "the implementation performed an action the spec forbids at this state (or reached a state violating an invariant)" }
```

exit code: conformant = 0, nonconformant = 1, input/spec error = 2.

A versioned trace's declared `initial` state is compared against `init`'s own
computed initial state only when `init` fully determines one (every state
variable, not component-wise); when it does not, the trace's own `initial` is
used as the concrete starting point directly instead (see §1.3's
`Monitor::from_state` note, #519), and `initial_state_mismatch` is not
checked for that run at all — there is currently no per-component fallback
that still cross-checks a state variable init *does* fully determine while
skipping only the ones a free component shares a root with.

## 3. `fslc testgen` — generation of a conformance-test skeleton

```
fslc testgen <file.fsl> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [--allow-unwired] [-o <out>]   # default target pytest; default file test_<spec name lowercased>.py to stdout
```

The native generator has one normalized input adapter for every target. For an
ordinary source it validates Public Kernel v1, then combines its spec/action/
parameter metadata with language-independent scenario JSON and the versioned
`testgen-trace.v1` conformance trace (`initial`, then ordered
`action`/`params`/`expected` steps). Pytest,
Vitest, Swift, Kotlin, Dart, and PHPUnit consume only this adapter; no emitter
receives the private `KernelModel` or AST. Incompatible Kernel schema ids or
versions, malformed vectors, mismatched spec names, and unknown targets fail
closed.

The testgen trace is intentionally a distinct fixed-seed, single-path public
contract rather than the exhaustive `fslc conformance` BFS corpus.
Substituting the latter would turn O(100 × enabled actions) generation into a
potential state-space expansion, change Option JSON shapes, and perturb stable
target output ordering. Its schema is
`schemas/fslc/kernel/testgen-trace.v1.schema.json`; it records its own and the
Kernel schema versions plus the spec identity. The adapter cross-checks every
state field, action, and exact parameter set against Public Kernel v1 before an
emitter runs. The frozen Python implementation remains a compatibility reference
and is unchanged.

Public Kernel v1/v2 deliberately reject `compose` until component file identities
can be retained truthfully. To preserve the independent compose/testgen contract,
the CLI selects an explicit compose constructor before export and supplies only
checked spec/state/action/parameter names and declaration order. It converges on
the same normalized input and never exposes expressions or AST nodes to emitters.
This bridge is retired when Public Kernel supports truthful compose provenance;
it is not a catch-on-export-error fallback.

**Native source-header contract (#1424).** Source basenames in every target's
headers share one visible spelling: backslash, both quotes, `*`, `/`, `?`, `>`,
and ASCII controls become lowercase `\xNN`. Other characters, including
non-ASCII, remain unchanged. Trusted template slots are filled first; the escaped
source name is inserted last and is never expanded again, even if it contains a
slot name. This prevents a basename from closing a docstring/comment or switching
PHP context. Ordinary output outside the Kotlin helper and its comment is
unchanged.

Native pytest strings escape controls (#1424): `\n`, `\r`, `\t`, otherwise
lowercase `\xNN`; quotes/backslashes remain escaped.

The output is a **self-contained pytest file** (the primary dependencies are
`fslc.runtime` and `pytest`. In addition, only the standard library needed for
wiring and replay may be imported — `random` (below, 3.) for the fixed-seed
pseudorandom walk and `pathlib` for resolving the SPEC path):

1. **Adapter stub**: a class for the user to wire up the implementation.
   ```python
   class Adapter:
       """Connect your implementation to the spec actions/state."""
       def reset(self): raise NotImplementedError
       def step(self, action: str, params: dict): ...   # drive the implementation for one action
       def observe(self) -> dict: ...                   # project the implementation's state into the spec's state shape
   ```
2. **Scenario replay test** (run the scenarios mechanism at testgen time and
   embed it): feed each scenario's steps to the Adapter, and assert that after
   each step `observe()` matches **only the fields mentioned** in `expected_states[i]`.
   If some `reachable` targets are not witnessed at the requested depth, testgen
   still embeds the witnessed scenarios and returns warning JSON naming each
   missing target with a depth hint. `--strict` restores all-or-nothing
   `reachable_failed`. A genuine `violated`/`reachable_failed` verdict from the
   underlying scenarios machinery — the spec itself has a bug, not a testgen
   input problem — is returned verbatim (`result`, exit code, and trace
   unchanged), the same envelope `fslc verify`/`fslc scenarios` return for the
   identical spec; it is never re-wrapped as a generic exit-2 spec error (a
   native-only regression fixed in issue #472, see `rust/fslc/src/main.rs`
   `run_testgen`/`run_domain_testgen`).
3. **Random-walk conformance test**: with the concrete Monitor as the oracle, choose actions
   from `mon.enabled()` by pseudorandom (fixed seed, `random.Random(0)`) for
   N=100 steps, and on every step assert `adapter.step(...)` → `observe() == mon.state`.
   If the Monitor side produces a violation (invariant etc.), distinguish it in
   the fail message as **a bug in the spec itself**.

   **Superseded on the native path by issue #843: the native CLI refuses to
   generate at all when the fixed-seed walk violates.** The clause above is
   sound only for a target whose walk runs live against the Monitor at test
   time, i.e. `--target pytest`. The five baked targets embed
   `(action, params, expected_state)` as a static fixture, and the Monitor
   rolls a violating step back, so there is no run-time oracle left to
   "distinguish in the fail message" — the rolled-back state was recorded as
   that step's `expected`, asserting a no-op the spec never states. That made a
   conforming implementation fail and an implementation that silently did
   nothing pass.

   The native fix is uniform across all six targets rather than pytest-only,
   because #843's required target-parity control is that one spec and seed
   yield the *same* conclusion everywhere; generating for pytest while refusing
   the baked five would re-break parity in the opposite direction. A walk
   violation is therefore reported as `result:"violated"` with `verify`'s
   envelope, exit code, property, location, and replayable trace, and no
   harness is written (see §3.0 below). This resolves the conflict between this
   clause and #843's control by adopting the stricter behavior on the
   authoritative implementation; the clause remains an accurate description of
   the frozen Python reference, which behaves differently (see §3.0).

#### 3.0 Walk-violation behavior differs across implementations (issue #843)

The fixed-seed walk is a concrete Monitor run capped at 100 steps and is **not**
bounded by `--depth`, so it can reach a violation the bounded verification
`testgen` performs first proved absent within `depth`. The three observable
behaviors for that input are:

| Implementation | Behavior on a violating fixed-seed walk |
|---|---|
| Frozen Python (`src/fslc/testgen.py`, `_bake_random_walk`) | bakes the violation-free prefix only and exits 0 |
| Native Rust before #843 | baked the full 100 steps with the rolled-back state as `expected` — the defect |
| Native Rust after #843 | writes no harness; `result:"violated"`, exit 1, `verify`'s envelope and trace |

The frozen Python never had the defect: it inspects the step result and breaks
before appending. Its truncation is nonetheless a *different* answer from the
native one, and it is silent — a caller cannot tell a spec bug from a short
walk. The native path fails closed instead, because a green harness generated
from a spec with a reachable violation is the false-negative direction
`AGENTS.md` ranks above a crash.

This divergence is deliberate and is **not** a parity defect to be allowlisted:
the Rust workspace is the authoritative implementation and the Python package
is a frozen compatibility reference (`AGENTS.md`). No parity gate compares
walk-violation behavior across the two implementations today, so nothing
enforces either side; this table is the record.
4. While the Adapter is unimplemented (NotImplementedError), every test
   **fails** with `pytest.fail(ADAPTER_NOT_WIRED, pytrace=False)`, whose message
   starts `adapter not wired`. `--allow-unwired` restores the earlier
   `pytest.skip` (see §3.6, issue #1200).

   **Superseded by issue #1200.** This item used to make every test
   `pytest.skip` "so that pytest does not error even right after generation".
   That made a suite whose adapter was never wired exit 0, and `fslc chain`
   reported that green as a passing `[impl]` layer, so a spec-to-implementation
   claim held without the implementation being called once.

### 3.1 `--target vitest` (TypeScript / Vitest)

The Vitest emitter renders the **same scenarios** to a self-contained TypeScript
file with the same `reset`/`step`/`observe` `Adapter` contract. Parts 1, 2, and 4
above port directly: an `Adapter` interface + `makeAdapter()` stub, deterministic
scenario tests (`assertPartial`), forbidden-rejection tests (`assertRejected`), and
the unwired behaviour of §3.6 (by default a `scenario()` wrapper throws
`adapter not wired` from every test; with `--allow-unwired` a top-level guard
flips `test` to `test.skip`).

**Native integer precision (#1424).** Generation rejects integers outside
±(2^53−1) before emitting an artifact, rather than writing a lossy JavaScript
number. The existing semantic-error envelope (exit 2) names the value and its
input-vector path, including forbidden-step params and rejection data. This
applies with or without `--allow-unwired`.

Part 3 — the random walk — is the one real design point, because TypeScript has no
`Monitor`. The chosen approach **bakes the trace at generation time**: the Python
Monitor runs the fixed-seed (`Random(0)`) walk and the resulting
`(action, params, expected_state)` sequence is embedded as a static fixture; the
Vitest test only replays it and asserts. Rationale:

- The walk is already deterministic (`Random(0)` + deterministic Monitor), so baking
  under the same seed yields the identical trace — equal coverage, nothing lost.
- It keeps the **single independent oracle** invariant: there is still exactly one
  Monitor (in Python), not a second reimplementation in TypeScript.
- The generated file is `fslc`-free at runtime — no shell-out, no Python dependency.

Rejected alternatives: (b) shelling out to `fslc` from the TS test (adds a runtime
coupling), and (c) porting `Monitor` to TS (duplicates the dual-evaluator surface
that the agreement/oracle tests exist to protect). Output defaults to
`<spec>.test.ts`.

### 3.2 `--target swift` (Swift Testing)

The Swift emitter renders the same scenarios to a self-contained Swift Testing file
(`import Testing` / `@Test` / `#expect` / `#require` — **not XCTest**), reusing the
language-independent baked walk. The Swift-specific points:

- **Dynamic dict + equality.** `params`/`observe()` are `[String: Any]`. `Any`
  values have no usable `==`, so the harness bundles `fslEqual` (a deep equality over
  the JSON-normal world: `Bool`/`Int`/`Double`/`String`/`FSLNull`/`[Any]`/`[String:
  Any]`, with `Int` and `Double` kept distinct) and an `assertPartial` that recurses
  by the expected keys and asserts only the fields the spec mentions.
- **Null.** An Option `None` bakes as `FSLNull.instance`, a one-line sentinel struct,
  so the generated file depends only on `Testing` (no Foundation/`NSNull`).
- **Fail-when-unwired (§3.6).** `makeAdapter()` throws `FSLNotWired.notWired`
  until wired, and its `description` is the `adapter not wired` message, so every
  `@Test` fails. With `--allow-unwired` each `@Test` instead carries
  `.enabled(if: isAdapterWired())`, so the suite is *disabled* until an adapter is
  connected (the pre-#1200 behaviour).
- **Literals.** `_swift_literal` renders int as `Int`, float as `Double` (always with
  a decimal point), bool/null per above, and strings with Swift escape rules
  (`\u{XX}`, which differ from JSON). The baked walk is an inline labelled-tuple
  array `[(action:params:expected:)]` (kept local to the test, so no global
  non-`Sendable` state). Output defaults to `<SpecName>ConformanceTests.swift`.

Rejected alternatives mirror Vitest's: no shell-out to `fslc`, no second `Monitor`
port. An `Encodable` generated-struct model was considered over dict-plus-helper but
deferred — the dict keeps the first version small and matches the language-independent
scenario JSON directly.

### 3.3 `--target kotlin` (kotlin.test)

The Kotlin emitter renders the same scenarios to a self-contained kotlin.test file,
again reusing the baked walk. The choices:

- **Framework: kotlin.test** (over JUnit5 / Kotest). It is multiplatform and on the
  JVM delegates to JUnit, so the generated file carries the lightest dependency. The
  imports are fixed to `kotlin.test.{Test, assertEquals, assertFalse, assertNotNull,
  assertTrue}`.
- **Dynamic dict + integral equality (#1424).** `params`/`observe()` are
  `Map<String, Any?>`. `assertPartial` recurses by expected map keys, asserting
  only the fields the spec mentions. Before leaf `assertEquals`, both sides are
  normalized recursively through List/Map values: Byte/Short/Int/Long become Long.
  List order/length and exact map keys/size inside lists remain significant;
  Float/Double and other non-integral leaves are unchanged and remain distinct.
  Adapters should read numeric params through `Number`, not cast them to `Int`:
  generated params can contain Int or Long depending on magnitude.
- **Fail-when-unwired (§3.6).** `makeAdapter(): Adapter?` returns `null` until
  wired and each `@Test` starts `val a = makeAdapter() ?: fail(ADAPTER_NOT_WIRED)`.
  kotlin.test has no portable runtime skip (no `assumeTrue`), so the only way to
  "skip" would be the early `?: return` the target used before #1200, under which
  an unwired suite passed. `--allow-unwired` is therefore rejected for Kotlin
  (exit 2) instead of generating that silent green.
- **Native literals (#1424).** Int-range integers remain unsuffixed; wider i64
  values use `L`, and i64 minimum uses `Long.MIN_VALUE`. Floats render as `Double`,
  and other literals use `null`, `listOf`/`mapOf` (empty ones carry explicit type
  args), and Kotlin string escapes — notably `$` must be escaped (string templates).
  The baked walk is a `List<Triple<String, Map<String, Any?>, Map<String, Any?>>>`. Output defaults to
  `<SpecName>ConformanceTest.kt`.

(No `swiftc -parse`-style syntax gate in tests: kotlinc has no dependency-free
parse-only mode — a real compile needs kotlin-test on the classpath — so the Kotlin
tests assert on harness shape and the baked walk instead.)

### 3.4 `--target dart` (package:test)

The Dart emitter renders the same scenarios to a self-contained `package:test` file
(which also runs under `flutter test`), reusing the baked walk. The choices:

- **Equality is the catch.** `params`/`observe()` are `Map<String, dynamic>`, and
  Dart's `==` is *reference* equality on `List`/`Map`, not structural. Rather than
  pull in `package:collection`'s `DeepCollectionEquality`, `assertPartial` recurses
  by the expected keys and compares leaves/sequences with the `equals` matcher, which
  *is* deep and is re-exported by `package:test` — so the generated file's only
  dependency stays `package:test`.
- **int/float.** Unlike PHP, Dart treats `1 == 1.0` as true, so `_dart_literal`
  renders the right syntax (`1` vs `1.0`) but the values compare equal — this is Dart
  semantics, not a fidelity gap.
- **Fail-when-unwired (§3.6).** `makeAdapter()` throws an `UnimplementedError`
  carrying the `adapter not wired` message, so every test fails until wired. With
  `--allow-unwired`, `package:test`'s static `skip:` argument is used instead: a
  top-level `_adapterWired()` probe runs once in `main()` and every
  `test(..., skip: wired ? null : 'Adapter not wired')` is conditionally skipped.
- **Literals.** Strings are single-quoted with `$` escaped (interpolation); empty
  collections carry explicit type args (`<String, dynamic>{}`); the baked walk is a
  `List<Map<String, dynamic>>` of `{action, params, expected}`. Output defaults to
  `<spec_name>_conformance_test.dart` (snake_case + the runner's `_test.dart` suffix).

(No syntax gate in tests: `dart analyze` needs a pub package context, so there is no
clean dependency-free parse-only mode — the Dart tests assert on shape + baked walk.)

### 3.5 `--target phpunit` (PHPUnit)

The PHPUnit emitter targets PHP 8.1+ / PHPUnit 10+ (`declare(strict_types=1)`),
reusing the baked walk. The choices:

- **Strict leaf equality is the whole point.** PHP's loose `==` makes `0 == "0"`
  and `1 == true` true, which is unsafe for a conformance test, so every leaf is
  compared with `assertSame` (`===`), and `_php_literal` keeps `int` (`1`) distinct
  from `float` (`1.0`) — `assertSame(1, 1.0)` is false.
- **Dict vs the numeric-key trap.** `params`/`observe()` are associative `array`s,
  but PHP coerces a numeric string key like `'0'` to int `0`, which collapses the
  map/list distinction. `assertPartial` therefore recurses by the *expected* keys
  (so a `Map<0..1, …>` matches by key — order-independent — and only the mentioned
  fields are asserted) and, for a genuinely list-shaped expected (`array_is_list`),
  also pins the length so sequences stay exact. Both sides see the same key coercion,
  so it cancels out.
- **Fail-when-unwired (§3.6).** `makeAdapter()` throws until wired; `setUp()`
  probes it and calls `$this->fail(...)` with the `adapter not wired` message, so
  every test fails. With `--allow-unwired` it calls `markTestSkipped` instead.
- **Literals.** Strings are single-quoted (PHP single-quotes interpolate nothing;
  only `\\`/`\'` are escaped); JSON arrays render to PHP lists, JSON objects to
  associative arrays. Native i64 minimum uses `PHP_INT_MIN` (#1424), avoiding a
  decimal magnitude that PHP lexes as a float. The baked walk is a
  `private const WALK`. Output defaults to
  `<SpecName>ConformanceTest.php` (PSR-4: class name = file name), test methods are
  `testScenario_<name>` / `testRandomWalkConformance`.

(Syntax gate in tests: `php -l` lints syntax without loading PHPUnit — a clean
dependency-free check like swiftc -parse, so the PHP tests run it when php is present
and skip it otherwise.)

### 3.6 Unwired adapters fail by default (issue #1200)

A generated test whose adapter is not wired never calls the implementation, so
its result says nothing about whether the implementation conforms. Before
#1200 every target skipped such tests (Kotlin returned early and passed), so a
forgotten wiring exited 0, and `fslc chain` lifted that exit 0 to a passing
`[impl]` layer. The default is now that **every generated test of every target
fails** while unwired, with a message that starts `adapter not wired: implement
makeAdapter() ...` (pytest: `implement Adapter.reset/step/observe ...`). This is
a breaking change of the generated text, shipped in a minor release (the
changelog fragment carries the migration note).

**Opt-in skip: `--allow-unwired`.** Skipping is an explicit choice at
generation time, for `fslc testgen` and `fslc domain testgen`. The opt-in
output is byte-identical to the pre-#1200 scaffold for pytest, Vitest, Swift,
Dart, and PHPUnit (`rust/fslc/tests/testgen_contract.rs` pins those digests),
so a project that wants the old file regenerates with the flag and gets exactly
it. A generation flag was chosen over a run-time environment variable for three
reasons:

- **One mechanism on every target.** Reading an environment variable needs a
  platform API the scaffolds deliberately avoid: Kotlin's common stdlib has
  none (`System.getenv` is JVM-only), Swift would need Foundation or libc
  imports the file does not carry, and Dart's `Platform.environment` is
  unavailable on the web. A flag changes only the generated text.
- **Visible in the artifact.** The choice is in the reviewed file, not in a CI
  setting that a reviewer of the test file cannot see.
- **No silent run-time downgrade.** A run-time switch can be set in one
  environment and forgotten; it would turn a failing unwired suite back into a
  green one without any file changing.

**Kotlin rejects `--allow-unwired`** (`result:"error"`, exit 2): kotlin.test has
no portable runtime skip, so the only opt-in available would be the early
return, which is the silent pass this section removes.

**What a skipping suite still means for `chain`.** `--allow-unwired` makes a
runner green while nothing ran. `fslc chain` therefore does not trust the exit
code alone: an `[impl]` layer names a JUnit `report`, and zero executed tests
is `indeterminate`, not `passed`; one without a report must opt into exit-code
evidence explicitly, and is `indeterminate` on exit 0 otherwise
(`DESIGN-layers.md` §7).

`fslc domain testgen` rewrites the guidance in its scaffold to
`fslc domain testgen --allow-unwired`, so the message names the command that
regenerates that file.

**Frozen Python reference.** `src/fslc/testgen.py` is unchanged and still
skips. The parked developer comparison `tools/check_rust_phase3_commands.py`
compares testgen bytes between the two implementations and now reports the
default scaffolds as different; the native default is the authoritative one
(`AGENTS.md`), and `--allow-unwired` is the native form equal to the Python
output. This is a deliberate divergence like §3.0, not a parity defect.

## 4. CLI / public API

- cli.py: add the `replay` / `testgen` subcommands. Existing commands unchanged.
- `fslc/__init__.py`: expose `from .runtime import Monitor`.

## 5. Constraints / out of scope

- leadsTo is not checked in the runtime monitor (a response property over a
  finite log cannot distinguish "not yet reached" from "violated." Stated in the
  replay output's `note`).
- The fair annotation has no effect at runtime.
- Monitoring under concurrent execution (thread safety) is out of scope (stated in the documentation).

## 6. Test plan (tests/test_runtime.py)

1. **Differential testing (most important)**: for every sample spec, replay
   `fslc verify`'s reachable witness trace (and scenarios' steps) with `Monitor`,
   and confirm that **each step's state exactly matches the witness's state**.
   (Mechanical verification of the semantic agreement between the Z3 evaluator
   and the concrete evaluator. A generalization of the existing test_scenarios §3.1.)
2. requires_failed: a step whose guard fails is ok: false / state unchanged.
3. Each violation of partial_op / type_bound / ensures / invariant is detected in
   Monitor too with the same kind (replay, one step at a time, the counterexample
   trace of a spec that becomes violated under verify, and the same kind is
   returned at the last step).
4. Non-deterministic init (a missing assignment) is a semantics error at Monitor construction.
5. replay: a conformant log / a nonconformant log (confirm failed_at_event and
   violation.kind) / array-form input.
6. testgen: the generated file is importable, fails if the Adapter is
   unimplemented (skips only under `--allow-unwired`), and with the Monitor wired in place of the Adapter, the
   "self-conformance" makes all tests pass (= verification that the generated
   skeleton works as-is).
7. enabled(): matches the expected instance enumeration in a known state.

## 7. Reflecting into documentation

- A "§10 bridge to the implementation" section in LANGUAGE.md: how to use Monitor
  / replay / testgen and the workflow (spec proved → testgen → implement Adapter → pytest).
- Add replay / testgen to the README's usage.
- An "implemented" note on the relevant items in DESIGN-v1.md §10.

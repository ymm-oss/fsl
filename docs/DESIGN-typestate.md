# FSL — `fslc typestate` (applicability judgment for state machine → phantom types) implementation design

Motivation: judge how soundly a design spec's state machine can be mapped to **typestate
(phantom types)** in a host language (TypeScript, etc.), and emit type templates only for the
range that can be mapped. The judgment itself is the deliverable — it mechanically separates,
from the spec, "what can be protected by types and what remains as a runtime / verification
obligation."

## 1. CLI / output

`fslc typestate <f> [--ts]` → `result:"typestate"`, exit 0. `--ts` emits to stdout only the
TypeScript for derivable entities. The output uses the same JSON envelope as the other commands.

## 2. Judgment: three-way classification per `(entity, action)`

- **`derivable`** — the from-state is a **local guard on the entity's own status field**
  (`requires e.status == S`) and the to-state is a local assignment. The runtime guard compiles
  soundly into a compile-time type. A compound guard's from-states are the states its **whole**
  formula implies, not the states that appear in any one operand: `and` unions the states each
  conjunct pins (one conjunct pinning a state is enough, since all conjuncts must hold). `or` is
  a disjunction, so its from-states are the **union** of what each disjunct implies — but only
  when **every** disjunct actually constrains the entity: `status == A or status == B` guarantees
  `status ∈ {A, B}` for every satisfying trace, so it stays `derivable` with `from` covering both.
  If any disjunct says nothing about the entity (e.g. an unrelated flag), that disjunct is
  satisfiable at *any* state, so the whole disjunction does not pin the entity at all and the
  transition cannot be `derivable` through that guard — `status == A or bypass` is not derivable,
  even though `status == A or status == B` is.
  A locally guarded action with no assignment to the entity is an explicit
  state-preserving self-loop. For one allowed state it reports `B → B`; for
  multiple allowed states it reports one self-loop per state. The TypeScript
  skeleton preserves the caller's exact phantom state with `S extends A | B`
  and returns `S`, rather than widening `A` to `A | B`. Actions that neither
  guard nor assign the entity are not operations of that entity's state machine.
- **`branching`** — the to-state is assigned only inside an `if` (data-dependent). It is exposed
  in the type, but the implementation bears a proof obligation of exhaustiveness (flagged).
- **`relational`** — there is **no local guard on the same entity** for the status assignment.
  Because the premise lives in an external structure (a queue, another entity), it cannot be
  carried by a phantom tag. It is not exposed in the type and is left with its reason
  (diagnostics) and the action's requirement ID (the business-layer `transition … by <actor>`,
  etc.).

## 3. The three corresponding state-machine forms

1. **enum value in a struct field** (`struct Order { status: St }`). This form is scoped by
   **field name and owning struct type together**: two different structs that both happen to
   declare a same-named field (e.g. `Order.status` and `Ticket.status`) are two independent
   entities and machines. A guard/assignment on one struct's field must never be attributed to
   another struct's same-named field just because the field names match.
2. **enum value in a state variable** (after business `process`/stages expansion).
3. **`Option<_>` slot** (none/some ≈ Empty/Filled).

## 4. applicability (per entity)

`full` only when all transitions, including locally guarded state-preserving
self-loops, are `derivable` (or `branching`). **It does not claim full by
dropping a transition it could not understand or a locally guarded operation
that happens to preserve state** (it errs on the sound side). If only some,
`partial`; if none, `none`.

## 5. Ripple / implementation

- The frozen Python reference remains in `src/fslc/typestate.py`; it is not the
  implementation path for native Rust releases.
- The native Rust CLI parses and checks the source once, projects it through
  `fsl_core::public_kernel_contract`, and passes only public Kernel JSON v1 to
  `fsl_tools::analyze_typestate`. The adapter validates `$schema` and
  `schema_version` before constructing the small typestate-specific view.
- The old native adapter that accepted `KernelModel` directly is deprecated and
  retired from the CLI path. Rust callers should export public Kernel JSON and
  call the JSON adapter; private AST/model shapes are not a generator contract.
- Public Kernel actions are normalized by name. Typestate restores declaration
  order from public source spans so the report and `--ts` output remain byte
  compatible with the established CLI contract.
- The verification engine and solver are unmodified. Reserved-word collisions
  for TypeScript identifiers remain escaped by the existing emitter.

## 6. Tests / related

The frozen reference behavior remains covered by `tests/test_typestate.py`.
Native coverage additionally checks the public-Kernel adapter, rejects unknown
schema versions, and compares both JSON and `--ts` bytes with v1 golden files.
In bridging formal specs to the implementation side's type system it is the same
family as DESIGN-bridge (testgen / Monitor) — whereas bridge emits "behavioral
conformance tests," typestate judges "the promotability of state premises into
**types**."

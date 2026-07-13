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
  soundly into a compile-time type.
- **`branching`** — the to-state is assigned only inside an `if` (data-dependent). It is exposed
  in the type, but the implementation bears a proof obligation of exhaustiveness (flagged).
- **`relational`** — there is **no local guard on the same entity** for the status assignment.
  Because the premise lives in an external structure (a queue, another entity), it cannot be
  carried by a phantom tag. It is not exposed in the type and is left with its reason
  (diagnostics) and the action's requirement ID (the business-layer `transition … by <actor>`,
  etc.).

## 3. The three corresponding state-machine forms

1. **enum value in a struct field** (`struct Order { status: St }`).
2. **enum value in a state variable** (after business `process`/stages expansion).
3. **`Option<_>` slot** (none/some ≈ Empty/Filled).

## 4. applicability (per entity)

`full` only when all transitions are `derivable` (or `branching`). **It does not claim full by
dropping a transition it could not understand** (it errs on the sound side). If only some,
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

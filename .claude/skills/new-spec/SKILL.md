---
name: new-spec
description: Author a new or substantially changed .fsl spec and take it through native check, bounded verification, induction where relevant, mutation, and vacuity evidence before completion.
---

# Author a meaningful FSL spec

Read `skills/fsl/reference.md` before writing syntax. Use the working-tree native prefix:

```text
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --
```

## Gate

1. `check <spec>` for syntax and semantic diagnostics.
2. `verify <spec> --depth 8` or a justified bound for bounded evidence.
3. `verify <spec> --engine induction` when the intended claim is unbounded.
4. `mutate <spec>` to measure whether properties constrain behavior.
5. `verify <spec> --vacuity error` where applicable.
6. Run the focused corpus/command test required by the spec's dialect or example family.

Do not hollow out a spec to dodge a counterexample. Confirm source requirements and exceptions before
inventing behavior. Keep confirmed assumptions with the spec using established metadata. If FSL is the
wrong tool for the behavior, say so rather than forcing a vacuous model.

Changing a corpus file may affect contract or compatibility artifacts. Use their owning generator only
after confirming the semantic change is intentional; never hand-edit a snapshot.

---
paths:
  - "**/*.fsl"
  - "docs/LANGUAGE.md"
  - "skills/fsl/**"
---

# FSL specification rules

- Read `skills/fsl/reference.md` before authoring syntax; do not rely on model memory for FSL grammar.
- Use the working-tree native CLI through
  `cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --`.
- Validate in order: `check`, bounded `verify`, induction where applicable, then mutation and vacuity.
- A green verdict is insufficient: preserve the modeled intent, reachable behavior, forbidden cases, and
  non-vacuity evidence.
- Keep confirmed assumptions in the `.fsl` using established tags or declaration metadata. Do not invent
  a parallel requirement-ID namespace.
- Any corpus behavior change needs an intentional regression case and review of the affected contract or
  compatibility artifacts. Never hand-edit a generated snapshot.

---
paths:
  - "rust/fsl-wasm/**"
  - "rust/fsl-solver-z3js/**"
  - "docs/intro/**"
  - ".github/workflows/pages.yml"
---

# Browser and generated-site rules

- The browser verifier runs in a dedicated Worker and shares the native CLI's Kernel and envelope
  contract; do not add an independent browser-only semantic implementation.
- Keep the WASM dependency boundary free of the native Z3 backend.
- Preserve COOP/COEP service-worker requirements, forced cancellation by Worker replacement, and healthy
  solver reinitialization.
- Generated site/Worker assets must be produced by their owning scripts. Do not hand-edit generated
  output to satisfy a snapshot.
- Run the npm browser gate in `rust/fsl-wasm` for Worker, bridge, or playground changes.

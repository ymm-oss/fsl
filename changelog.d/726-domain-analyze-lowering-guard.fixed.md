Fixed (#726): `fsl_tools::analyze_domain` now calls `domain_kernel_source`
before projecting a domain document, instead of projecting the raw
`DomainSpec` directly. `fslc domain analyze`'s accepted/rejected spec set is
now identical to `domain expand`'s, since both share `domain_kernel_source`:
this rejects not only the three #710/#711/#712 unlowerable constructs (a
top-level `await`, an `on_stale` policy, a `value_object` invariant), which
`check`/`verify` (including the wasm Worker's) already reject transitively
through `lower_domain`, but also a conflicting explicit effect-outcome role,
a duplicate or empty enum declaration, and any failure `domain_kernel_source`'s
own kernel-text rendering raises (for example an unsupported Map/container
default shape, or a reference to an unknown domain type). `analyze` previously
accepted all of these with `result:"analyzed"`/exit 0; it now matches `domain
expand`'s diagnostic byte-for-byte for every one of them. A well-formed
domain spec still analyzes successfully.

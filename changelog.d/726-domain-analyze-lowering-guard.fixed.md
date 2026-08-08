Fixed (#726): `fsl_tools::analyze_domain` now calls the same
lowerable-construct guard `domain_kernel_source` already calls before
projecting a domain document, instead of projecting the raw `DomainSpec`
directly. `fslc domain analyze` now rejects the same three unlowerable
constructs (#710/#711/#712: a top-level `await`, an `on_stale` policy, a
`value_object` invariant) that `check`/`verify` (including the wasm
Worker's) already reject transitively through `lower_domain`, and that
`domain check`/`domain expand` already reject directly through
`domain_kernel_source`. `analyze` previously accepted all three with
`result:"analyzed"`/exit 0; it now matches `domain expand`'s diagnostic
byte-for-byte and `check`'s diagnostic kind, location, and exit code. A
well-formed domain spec still analyzes successfully.

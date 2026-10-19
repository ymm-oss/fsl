Fixed (#1091): a requirements document's `terminal_rule` claim is now
attributed to its `terminal` block. `fslc document claims` reports it
`source_backed` with the block's `source` location instead of `unknown` with
no location, so a spec that declares `terminal` keeps
`provenance.completeness: complete`, and `fslc document generate` no longer
adds the "provenance information is incomplete" note for it. Claim and claim
set digests do not change. Migration: a requirements document generated before
this fix for a spec that declares `terminal` carries that note, so
`fslc document check` (and `fslc approval` over a `requirements_document`)
now reports it as drifted (`edit_outside_slot`); regenerate it with
`fslc document generate`.

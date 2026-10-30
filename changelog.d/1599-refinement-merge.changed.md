Refinement rejects conflicting same-named type definitions, including unequal
resolved domain bounds, at the refinement/implements declaration (type, exit 2).
Symmetry annotations do not participate; equal definitions retain the impl entry.
With preserved progress, different enums may not share member spellings, even at
equal ordinals; without progress, typed conversions/abstractions remain accepted.
Breaking removal: refine_mapping_boundary changes from refinement_failed /
abs_state_mismatch (exit 1) to error / type (exit 2). Use distinct type names.
Mutants changing the bounds of a type named identically in the abstraction are
invalid under #1472's static-type classification, not oracle errors or kills.
The rule includes entity instances: impl_spec_extra's default check/verify now
rejects Item=1 versus Item=3 with type/exit2 instead of refinement_failed/exit1.
Audit a52bef4931adc67ea2b1fdca827e2318b4ae25ab→9eaaea7d1c526ede6a13408ea414616ddadccf69:
check610 corpus/39 next controls,60 mappings,20 inline/2 expression triples/39 cells,
full JSON/stderr/exit twice. Existing corpus check ok→error is empty. Inline
removals: original D-bound issue1296/1302, inline collision and governance collision;
verify follows. Refine removals: cases1/2a/3a/3b, domain_bounds/domain_lower,
kind_change/struct_named_type, matching-ordinal/chain and shared-member expression
progress. Renamed controls pass. Original2 runtime/2 CLI/3 mutation diagnostics
now prefer declaration rejection; invalid/gates stay unchanged. Authored1:1 is
the full refinement keyword token, not fallback; map companions retain map spans.
Existing type errors gain loaded operand identities; boundary/entity change class.

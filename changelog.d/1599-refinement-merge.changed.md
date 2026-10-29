Refinement rejects conflicting same-named type definitions, including unequal
resolved domain bounds, at the refinement/implements declaration (type, exit 2).
Symmetry annotations do not participate; equal definitions retain the impl entry.
With preserved progress, different enums may not share member spellings, even at
equal ordinals; without progress, typed conversions/abstractions remain accepted.
Breaking removal: refine_mapping_boundary changes from refinement_failed /
abs_state_mismatch (exit 1) to error / type (exit 2). Use distinct type names.
Mutants changing the bounds of a type named identically in the abstraction are
invalid under #1472's static-type classification, not oracle errors or kills.

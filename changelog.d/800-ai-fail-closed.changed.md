Changed (#800)!: `fslc ai check`, `fslc ai compat`, and `fslc ai replay` now
reject invalid `ai_component` declarations (undeclared authority tools and
unknown `check hard` rule names, plus checked project parsing for `ai replay`)
with exit 2 instead of returning `ai_project_analyzed`,
`compat_profile_generated`, or `replay_conformant` on false greens.

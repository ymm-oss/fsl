Documented: the distributed FSL skills no longer contradict the parser or
each other. `number` is documented as requirements/kernel-only (a `business`
spec that declares one is a parse error); `if c then a else b` is valid in any
expression position; requirements `terminal` uses `stage(c)`; `@requirement`
and `@undecided` are the taught tag forms (the `"ID: text"` slot is
`legacy_string_metadata`); a kernel spec cannot declare `acceptance` or
`forbidden`; the `decreases`-inside-`forall` diagnostic is quoted as the
parser emits it; the AI-project examples show only the lines the scanner
reads; broken `§` cross-references now name the file.

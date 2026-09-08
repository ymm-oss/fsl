Fixed (#1001): `fslc check|verify --strict-tags --requirements` now counts a requirement
ID linked by a typed `@requirement(...)` annotation on an `init` block or on an
`acceptance`/`forbidden` block as referenced, instead of reporting it as
`unreferenced_requirement`; a genuinely unreferenced ID still warns.

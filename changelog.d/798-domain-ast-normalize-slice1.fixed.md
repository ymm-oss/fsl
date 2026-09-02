Fixed (#798, slice 1): `domain_kernel_source` now normalizes domain expressions
through scope-aware AST composition instead of blind `str::replace`, so path B
agrees with `lower_domain` on generated-name misuse and command-input shadowing.
`KNOWN_DIVERGENT_DOMAIN_FIXTURES` is empty; slice 2 retires the legacy string
path and #796 CLI suppression.

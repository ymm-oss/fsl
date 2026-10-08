Fixed (#1190): `fslc kernel` failed with `semantics: public Kernel cannot type
identifier 'k'` on a statement-level `forall k: K { … }` whose body had a
partial operation that read the binder (`m[k] = s.at(k)`, `m[k] = 1 / (k + 1)`,
or a correctly guarded `m[k] = if k < s.size() then s[k] else 0`), although
`verify` and `explain` handled the same spec. The Public Kernel's
`partial_operations` now expands a statement-level `forall` like a quantifier:
one entry per candidate for each partial operation in the body and in the
binder's `where` (previously not listed at all), with the binder replaced by
the candidate and the failure condition guarded by the candidate's membership
and `where`. A `forall` body operation that does not read the binder, which
was listed once with an unguarded failure condition, is now listed once per
candidate. A statement-level `forall` with no partial operation in its
binder, `where` or body is still not expanded and lists nothing, so
`forall k in 0..x { m[k] = 1 }` with a non-constant bound keeps producing a
Kernel. One that does have a partial operation now needs constant bounds,
like a quantifier: `forall k in 0..x { m[k] = 2 / x }`, which used to list
the division once unguarded, now fails with `'x' is not an integer const`
(the BMC engine already rejected it the same way). Commands built on the
Public Kernel (`testgen`, `testplan`, `document`) no longer stop with the
"cannot type identifier" error. Verification verdicts are
unchanged, and no spec or fixture in the repository corpus changes its
`fslc kernel` output.

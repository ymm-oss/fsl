Fixed (#1451): explicit and Monitor now diagnose init reads of never-assigned
state roots as unsupported free initial values (nondeterministic init), with
guidance to use `--engine bmc`, instead of claiming the root is read before
assignment. Reported names retain compose prefixes. This intentionally changes
the public message only: semantics/exit 2, assigned-root early-read rejection,
check acceptance of free roots, BMC/induction symbolic values, and refinement's
free-root enumeration remain unchanged. Auto still falls back to BMC and
forwards the new diagnostic in `engine_fallback.reason`; explicit gains no
free-root enumeration.

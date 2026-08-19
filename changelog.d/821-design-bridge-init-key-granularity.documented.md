Documented (#821): corrected `docs/DESIGN-bridge.md` §1.3's init-determinism
bullet, whose limiting clause ("... rather than a `forall`-bound variable")
described the pre-fix explicit-engine behavior instead of the per-concrete-key
contract `docs/LANGUAGE.md` already stated. The bullet now says a later flat
`m[K] = ...` write to a key a `forall` bulk assignment already covered is a
duplicate too, matching the corrected runtime check.

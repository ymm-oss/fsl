Fixed (#1521): `refine` refuses incomplete assigned init roots on both its
implementation and abstraction free-root paths instead of silently using defaults
for unwritten fields, map keys, branches, or empty/filtered forall domains.
The error explains that bmc and induction still return verdicts using free values.
Never-assigned scalar roots remain enumerated, provably nonempty singleton filters
remain accepted, and check, verification engines, and Monitor behavior are unchanged.

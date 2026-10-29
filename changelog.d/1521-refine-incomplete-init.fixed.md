Fixed (#1521): `refine` refuses incomplete assigned init roots on both its
implementation and abstraction free-root paths instead of silently using defaults
for unwritten fields, map keys, branches, or unproved forall coverage.
The error explains that bmc and induction still return verdicts using free values.
Never-assigned scalar roots remain enumerated. Singleton fixed-target assignments
with a static, state-independent filter proven nonempty remain accepted;
state-reading filters do not prove nonemptiness and cannot establish full coverage.
Check, verification engines, and Monitor behavior are unchanged.

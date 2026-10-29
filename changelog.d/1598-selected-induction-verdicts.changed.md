Selected induction now establishes every implicit bound assumed by its step in
the base case, even with `--property P`. This intentionally changes these verdicts:

- Exit 0 false `proved` / `unbounded` → exit 1 `violated` / `type_bound`, naming
  the failing `_bounds_*`.
- Exit 2 `error` / `kind: "semantics"` for an overflowing Seq frame → exit 1
  `violated` / `type_bound` (step 0 for deterministic initial overflow).
- The shared base also changes `sweep --engine induction --property P` from
  exit 0 `sweep_passed` to exit 1 `sweep_failed` (with `v: 0..3` initialized to
  5 and `--depth 2..3`, it reports `_bounds_v`@0), and `verify --lemma` targets from exit 0 `proved` to exit 1
  `violated` / `type_bound`.
- `unknown_cti` or `violated P` at the same or a later step → `violated _bounds_*`
  at the first failing base step. Same-step examples: `n: 0..1`, init 0,
  increment, `P { n <= 1 }` at depth 2 changes `P`@2 to `_bounds_n`@2; a Set
  initial copy with `K = 0..1`, `init { x = 2 t = Set { x } s = t }`,
  `P { not s.contains(2) }` changes `P`@0 to `_bounds_s`@0 (both remain exit 1
  `violated`, now `type_bound`).

Sibling user invariants stay unselected; standalone BMC/explicit selection is
unchanged.

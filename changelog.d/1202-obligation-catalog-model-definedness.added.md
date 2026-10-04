Added (#1202, #1221): the obligation catalog also lists `leadsTo` deadlines,
the ranking-proof rows of a `decreases` clause (measure definedness, lower
bound, no deadlock, one step row per action, and the `helpful` fairness and
stickiness rows), and two model-definedness rows on every site: `NoOverflow`
for `i64` overflow and `KeyInDomain` for a `Map` index outside its finite key
domain. A `KeyInDomain` row is marked statically vacuous only when the index's
static type lies inside the key type. CLI, Worker and Public Kernel output are
unchanged.
